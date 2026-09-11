use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use common::{pgp, sealed};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::ca::{self, Ca};
use crate::db::{self, BuildRecord};

pub struct NewBuildOutput {
    pub build_id: String,
    pub client_binary: PathBuf,
    pub artifact_path: PathBuf,
}

pub fn new_build(
    conn: &Connection,
    master_key: &[u8; sealed::KEY_LEN],
    ca: &Ca,
    ca_cert_pem: &str,
    uid: &str,
    artifact: &Path,
    server_url: &str,
    data_dir: &Path,
    target: Option<&str>,
    no_antivm: bool,
    debug: bool,
    obfs: bool,
) -> Result<NewBuildOutput> {
    let build_id = gen_build_id(uid);
    let passphrase = gen_passphrase();

    let (pub_armored, priv_armored) = pgp::generate_keypair(uid, &passphrase)?;

    let plaintext = fs::read(artifact)
        .with_context(|| format!("failed to read artifact {}", artifact.display()))?;
    let ciphertext = pgp::encrypt_to_public(&plaintext, &pub_armored)?;

    let artifacts_dir = data_dir.join("artifacts");
    fs::create_dir_all(&artifacts_dir).context("failed to create artifacts dir")?;
    let artifact_path = artifacts_dir.join(format!("{build_id}.pgp"));
    fs::write(&artifact_path, &ciphertext).context("failed to write encrypted artifact")?;

    let issued = ca::issue_client_cert(ca, &build_id)?;

    let record = BuildRecord {
        id: build_id.clone(),
        created_at: now_string(),
        status: "built".to_string(),
        artifact_path: artifact_path.to_string_lossy().to_string(),
        pub_armored: pub_armored.clone(),
        priv_armored,
        passphrase,
        client_cn: build_id.clone(),
    };
    db::insert_build(conn, master_key, &record)?;

    let client_binary = compile_client(
        &build_id,
        server_url,
        ca_cert_pem,
        &issued.key_pem,
        &issued.cert_pem,
        &pub_armored,
        data_dir,
        target,
        no_antivm,
        debug,
        obfs,
    )?;

    Ok(NewBuildOutput {
        build_id,
        client_binary,
        artifact_path,
    })
}

fn compile_client(
    build_id: &str,
    server_url: &str,
    ca_cert_pem: &str,
    client_key_pem: &str,
    client_cert_pem: &str,
    pub_armored: &str,
    data_dir: &Path,
    target: Option<&str>,
    no_antivm: bool,
    debug: bool,
    obfs: bool,
) -> Result<PathBuf> {
    let workspace_root = workspace_root()?;
    let staging = data_dir.join("staging").join(build_id);
    fs::create_dir_all(&staging).context("failed to create staging dir")?;

    let ca_path = staging.join("ca.pem");
    let identity_path = staging.join("identity.pem");
    let pubkey_path = staging.join("pubkey.asc");
    fs::write(&ca_path, ca_cert_pem).context("failed to stage ca cert")?;
    fs::write(
        &identity_path,
        format!("{client_key_pem}\n{client_cert_pem}"),
    )
    .context("failed to stage client identity")?;
    fs::write(&pubkey_path, pub_armored).context("failed to stage build public key")?;

    let ca_path = fs::canonicalize(&ca_path).context("failed to resolve staged ca cert")?;
    let identity_path =
        fs::canonicalize(&identity_path).context("failed to resolve staged client identity")?;
    let pubkey_path =
        fs::canonicalize(&pubkey_path).context("failed to resolve staged build public key")?;

    let mut cmd = Command::new("cargo");
    cmd.current_dir(&workspace_root)
        .arg("build")
        .arg("-p")
        .arg("client")
        .env("OMC_BUILD_ID", build_id)
        .env("OMC_SERVER_URL", server_url)
        .env("OMC_CA_CERT", &ca_path)
        .env("OMC_CLIENT_IDENTITY", &identity_path)
        .env("OMC_BUILD_PUBKEY", &pubkey_path)
        .env("OMC_DEBUG_CLIENT", if debug { "1" } else { "0" });

    if !debug {
        cmd.arg("--release");
    }

    if no_antivm {
        cmd.arg("--no-default-features");
    }

    if obfs {
        cmd.arg("--features").arg("obfs");
    }

    if let Some(t) = target {
        cmd.arg("--target").arg(t);
    }

    let status = cmd.status().context("failed to invoke cargo build")?;
    if !status.success() {
        bail!("client build failed with status {status}");
    }

    let mut built = workspace_root.join("target");
    if let Some(t) = target {
        built = built.join(t);
    }
    built = built.join(if debug { "debug" } else { "release" });
    let exe = if target.map(|t| t.contains("windows")).unwrap_or(false) {
        built.join("client.exe")
    } else {
        built.join("client")
    };
    if !exe.exists() {
        bail!("expected client binary not found at {}", exe.display());
    }

    let out_dir = data_dir.join("clients").join(build_id);
    fs::create_dir_all(&out_dir).context("failed to create client output dir")?;
    let dest = out_dir.join(exe.file_name().unwrap());
    fs::copy(&exe, &dest).context("failed to copy client binary")?;
    Ok(dest)
}

fn workspace_root() -> Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .ok_or_else(|| anyhow!("failed to locate workspace root"))
}

fn gen_build_id(uid: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(uid.as_bytes());
    hasher.update(nanos.to_le_bytes());
    let digest = hasher.finalize();
    format!("build-{}", hex::encode(&digest[..8]))
}

fn gen_passphrase() -> String {
    let salt = sealed::random_salt();
    let more = sealed::random_salt();
    format!("{}{}", hex::encode(salt), hex::encode(more))
}

fn now_string() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string())
}
