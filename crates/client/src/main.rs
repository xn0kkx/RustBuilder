use std::path::PathBuf;
use std::time::Duration;

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

fn fetch_artifact(client: &Client, server: &str, build_id: &str) -> Result<Vec<u8>> {
    let url = format!("{server}/builds/{build_id}/artifact");
    let response = client
        .get(&url)
        .send()
        .with_context(|| format!("failed to request {url}"))?;
    if !response.status().is_success() {
        bail!("artifact request failed with status {}", response.status());
    }
    Ok(response
        .bytes()
        .context("failed to read artifact body")?
        .to_vec())
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

fn main() -> Result<()> {
    let args = Args::parse();
    let client = build_client()?;

    let ciphertext = fetch_artifact(&client, &args.server, &args.build_id)?;
    let key = fetch_key(&client, &args.server, &args.build_id)?;

    let plaintext = pgp::decrypt(&ciphertext, &key.priv_armored, &key.passphrase)?;

    std::fs::write(&args.out, &plaintext)
        .with_context(|| format!("failed to write {}", args.out.display()))?;

    println!("decrypted file written to {}", args.out.display());
    Ok(())
}
