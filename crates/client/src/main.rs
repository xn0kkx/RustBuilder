use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::ptr;

use anyhow::{bail, Context, Result};
use clap::Parser;
use common::pgp;
use common::proto::KeyResponse;
use reqwest::blocking::Client;
use reqwest::{Certificate, Identity};

include!(concat!(env!("OUT_DIR"), "/config.rs"));

#[derive(Parser)]
#[command(name = "client")]
struct Args {
    #[arg(long)]
    out: PathBuf,

    #[arg(long, default_value = SERVER_URL)]
    server: String,

    #[arg(long, default_value = BUILD_ID)]
    build_id: String,
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

/// Baixa o artefato cifrado em chunks, gravando o ciphertext (PGP) direto no arquivo
/// temporário `tmp`. reqwest (feature `gzip`) negocia `Accept-Encoding: gzip` e descomprime
/// de forma transparente, então o que chega ao disco já é o ciphertext PGP. O pico de RAM do
/// download fica limitado ao buffer de 64 KiB.
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

/// Baixa (em chunks, p/ o temp cifrado), descriptografa em memória e grava a saída.
fn run(client: &Client, args: &Args, tmp: &Path) -> Result<()> {
    download_artifact(client, &args.server, &args.build_id, tmp)?;

    let ciphertext = std::fs::read(tmp)
        .with_context(|| format!("failed to read temp file {}", tmp.display()))?;
    let key = fetch_key(client, &args.server, &args.build_id)?;

    let plaintext = pgp::decrypt(&ciphertext, &key.priv_armored, &key.passphrase)?;

    std::fs::write(&args.out, &plaintext)
        .with_context(|| format!("failed to write {}", args.out.display()))?;
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let client = build_client()?;

    let tmp = tmp_path_for(&args.out);
    let result = run(&client, &args, &tmp);
    // Remove o temp cifrado em qualquer caminho (sucesso ou erro); best-effort.
    let _ = std::fs::remove_file(&tmp);
    result?;

    println!("decrypted file written to {}", args.out.display());
    Ok(())
}
