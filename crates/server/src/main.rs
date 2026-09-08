mod ca;
mod db;
mod orchestrator;
mod tls;

use std::net::SocketAddr;
use std::path::PathBuf;
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
    },
    Serve {
        #[arg(long, default_value = "127.0.0.1:8443")]
        addr: SocketAddr,

        #[arg(long, default_values_t = [String::from("localhost"), String::from("127.0.0.1")])]
        san: Vec<String>,
    },
}

#[derive(Clone)]
struct AppState {
    conn: Arc<Mutex<Connection>>,
    master_key: Arc<[u8; common::sealed::KEY_LEN]>,
    data_dir: Arc<PathBuf>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tls::install_crypto_provider();

    let cli = Cli::parse();
    let passphrase = read_master_passphrase()?;

    let conn = db::open(&cli.db)?;
    db::init_schema(&conn)?;
    let meta = db::load_or_create_meta(&conn, &passphrase, ca::create_ca)?;

    match cli.command {
        Commands::NewBuild {
            artifact,
            uid,
            server_url,
            target,
            no_antivm,
        } => {
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
            )?;
            println!("build id:       {}", out.build_id);
            println!("client binary:  {}", out.client_binary.display());
            println!("encrypted file: {}", out.artifact_path.display());
            Ok(())
        }
        Commands::Serve { addr, san } => run_server(meta, conn, cli.data_dir, addr, san),
    }
}

#[tokio::main]
async fn run_server(
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
    require_cn(&peer, &id)?;

    let path = {
        let conn = state.conn.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        db::get_build_artifact(&conn, &id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    let Some(path) = path else {
        return Err(StatusCode::NOT_FOUND);
    };

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
    require_cn(&peer, &id)?;

    let secret = {
        let conn = state.conn.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        db::get_build_secret(&conn, &state.master_key, &id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    let Some((priv_armored, passphrase, client_cn)) = secret else {
        return Err(StatusCode::NOT_FOUND);
    };

    if client_cn != id {
        return Err(StatusCode::FORBIDDEN);
    }

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
