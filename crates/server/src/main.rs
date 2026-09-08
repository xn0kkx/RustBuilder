mod ca;
mod db;
mod orchestrator;
mod tls;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use axum::extract::{Extension, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use clap::{Parser, Subcommand};
use common::proto::KeyResponse;
use rusqlite::Connection;

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
            )?;
            println!("build id:       {}", out.build_id);
            println!("client binary:  {}", out.client_binary.display());
            println!("encrypted file: {}", out.artifact_path.display());
            Ok(())
        }
        Commands::Serve { addr, san } => run_server(meta, conn, addr, san),
    }
}

#[tokio::main]
async fn run_server(
    meta: db::Meta,
    conn: Connection,
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
    };

    let app = Router::new()
        .route("/builds/:id/artifact", get(artifact_handler))
        .route("/builds/:id/key", get(key_handler))
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

    // Stream o blob cifrado do disco em chunks (sem carregar tudo na RAM). O
    // CompressionLayer do router aplica gzip em streaming quando o cliente pede.
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

fn read_master_passphrase() -> Result<String> {
    if let Ok(p) = std::env::var("SERVER_MASTER_PASSPHRASE") {
        return Ok(p);
    }
    rpassword::prompt_password("Server master passphrase: ")
        .context("failed to read master passphrase")
}
