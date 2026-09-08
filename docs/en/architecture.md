# Architecture

## Overview

```
new build request ──▶ SERVER (Linux)
                      1. generate PGP keypair (rpgp)
                      2. generate per-build client cert, signed by the CA (rcgen)
                      3. encrypt the artifact to the public key  ──▶ ciphertext blob
                      4. seal private key + passphrase (XChaCha20-Poly1305) ──▶ SQLite
                      5. compile the client binary (cargo --target windows-gnu)
                         embedding: build id, server URL, CA cert, client cert+key
                              │
                              ▼
                      CLIENT.EXE (Windows, self-contained, per build)
                      a. connect over mTLS (presents the embedded cert)
                      b. GET the encrypted artifact
                      c. GET the build's private key
                      d. decrypt in memory and write the plaintext locally
```

The plaintext exists **only on the client machine**: in memory during decryption and in
the requested output file.

## Crates (Cargo workspace)

```
RustBuilder/
  Cargo.toml                 # [workspace] members = crates/*
  .cargo/config.toml         # mingw linker for the windows-gnu target
  crates/
    common/                  # shared library
      src/pgp.rs             # key generation, encrypt-to-public, decrypt
      src/sealed.rs          # Argon2id master key + XChaCha20-Poly1305 seal/open
      src/proto.rs           # serde request/response types
    server/                  # API + orchestrator (Linux binary)
      src/main.rs            # CLI (serve / new-build), axum routes, mTLS CN binding
      src/db.rs              # rusqlite schema + sealed columns
      src/ca.rs              # rcgen CA + certificate issuance
      src/orchestrator.rs    # generate key, encrypt artifact, invoke cargo build
      src/tls.rs             # rustls ServerConfig with client verification (mTLS)
    client/                  # per-build downloader (Windows binary)
      build.rs               # embeds the build assets via environment variables
      src/main.rs            # reqwest mTLS, fetch key+artifact, decrypt, write output
```

### `common`
Stateless library, used by both server and client.

- **`pgp.rs`**
  - `generate_keypair(uid, passphrase) -> (pub_armored, priv_armored)`: RSA 3072 with an
    encryption subkey; preferred algorithms AES-256 / SHA-256 / ZLIB.
  - `encrypt_to_public(plaintext, pub_armored) -> Vec<u8>`: ASCII-armored OpenPGP message.
  - `decrypt(ciphertext, priv_armored, passphrase) -> Vec<u8>`: accepts armored and binary.
- **`sealed.rs`**
  - `derive_master_key(passphrase, salt)`: Argon2id → 32-byte key.
  - `seal` / `open`: XChaCha20-Poly1305 with a random 24-byte nonce.
  - `seal_str` / `open_str`: string convenience, `zeroize`-ing the temporary buffer.
- **`proto.rs`**: `KeyResponse { build_id, priv_armored, passphrase }`, `BuildInfo`.

### `server`
Linux binary with two subcommands (`new-build`, `serve`). Details in [run.md](run.md) and
[api.md](api.md).

- **`orchestrator.rs`** — `new_build`:
  1. generate `build_id` (`build-` + hex of SHA-256(uid + nanos)) and a random passphrase;
  2. `generate_keypair` → seal private key + passphrase → insert the build row;
  3. `encrypt_to_public(artifact)` → write the ciphertext blob to `data/artifacts/<id>.pgp`;
  4. issue the client certificate (CN = build_id) signed by the CA;
  5. compile the client: `cargo build -p client --release [--target ...]` with the
     `OMC_BUILD_ID`, `OMC_SERVER_URL`, `OMC_CA_CERT`, `OMC_CLIENT_IDENTITY` variables, and
     copy the binary to `data/clients/<id>/`.
- **`tls.rs`** — rustls `ServerConfig` with `WebPkiClientVerifier` (requires a client
  certificate chaining to the CA). A custom `MtlsAcceptor` reads the client certificate's
  CN after the handshake and injects it as `Extension<PeerCn>`, allowing cert → build
  binding in the routes.
- **`ca.rs`** — creates/loads the CA and issues server certificates (`ServerAuth`, with
  SANs) and client certificates (`ClientAuth`, CN = build_id). `common_name_from_der`
  extracts the CN via `x509-parser`.
- **`db.rs`** — schema and helpers; every secret column goes through `sealed`.

### `client`
Windows binary generated per build.

- **`build.rs`** — reads the `OMC_*` variables (paths to PEM files) and generates, in
  `OUT_DIR`, a `config.rs` with `BUILD_ID`, `SERVER_URL` and `include_bytes!` of the CA
  cert, the mTLS identity (key + cert concatenated) and the **build's PGP public key**
  (`PUB_KEY`, from `OMC_BUILD_PUBKEY`). Without the variables it uses dev defaults, so the
  crate still compiles standalone.
- **`main.rs`** — builds a `reqwest::blocking::Client` with `use_rustls_tls()`, adds the
  embedded CA as a root and the embedded mTLS identity; performs `GET` for the artifact and
  the key; calls `common::pgp::decrypt`; writes the `--out` file. The reusable
  `upload_diagnostic` function is the reverse path: it encrypts a diagnostic file with the
  embedded `PUB_KEY` (`common::pgp::encrypt_to_public`), writes the ciphertext to a temp
  `<file>.part`, and `POST`s it to `/builds/:id/diagnostics` with a streaming (chunked) body.
  The `--upload <file>` flag is a thin wrapper over it for immediate use.
- **Anti-VM protection (optional).** The `antivm` feature (on by default) embeds the `antivm`
  library and invokes protection at the start of `main`, effective only on the Windows target
  (`#[cfg(all(windows, feature = "antivm"))]`). Disable it with `--no-default-features` (or
  `new-build --no-antivm`) for development testing. Details in [antivm.md](antivm.md).

## Trust flow (mTLS)

- A single **CA** (created on first use, private key sealed in the database) signs both the
  **server** certificate and the **client** certificates (one per build).
- The server **requires** a valid client certificate chaining to the CA (authenticated
  transport) and, in addition, verifies that the **client certificate CN == the build id**
  of the route. This prevents build A's client from fetching build B's key.
