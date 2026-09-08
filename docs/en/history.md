# History of what was done

Chronological order of the development steps and the decisions made.

## 1. Initial PGP downloader (single crate)

A Rust program that downloads a file over HTTPS and decrypts it **locally** with a PGP
key. Basis of today's `common::pgp::decrypt`.

- HTTP: `reqwest` (blocking) with `rustls-tls`.
- PGP: `pgp` (rpgp), pure Rust (avoids sequoia's C dependencies).
- Accepts ASCII-armored and binary messages (sniffs the `-----BEGIN PGP` prefix).
- Validated end-to-end: throwaway GPG key → encrypted file → download → decrypt →
  identical `diff`.

## 2. Evolution to server + client (build orchestration)

Requirement: a **different encryption key** per new build, stored in an **encrypted
database**; the server **orchestrates builds and connections**.

Agreed decisions:
- "build" = the server **compiles the client binary** per release, each with its own key.
- The server **generates the PGP keypair**, stores both halves sealed, and **delivers the
  private key to the authenticated client on demand**.
- Stack: **Rust + axum + SQLite with application-layer encryption** (Argon2id +
  XChaCha20-Poly1305).
- Client↔server auth: **mTLS** (the server's CA issues a per-build cert).

Implementation:
- Workspace with three crates: `common`, `server`, `client`.
- `common`: `generate_keypair`, `encrypt_to_public`, `decrypt`; `seal`/`open` sealing.
- `server`: CA (`rcgen`), database (`rusqlite` bundled), orchestrator (compiles the
  client), mutual TLS (`rustls` + `WebPkiClientVerifier` + a custom acceptor that reads the
  client certificate's CN), routes `/builds/:id/artifact` and `/builds/:id/key`.
- `client`: `build.rs` embeds build id, URL, CA and mTLS identity; `main.rs` fetches over
  mTLS and decrypts locally.

Validations:
- Clean debug and release builds, **zero warnings**.
- PGP roundtrip (keygen → encrypt → decrypt) confirmed by a temporary test.
- End-to-end: `new-build` → `serve` → client → **identical** `diff`.
- Security: no client cert → refused handshake; valid cert on wrong id → **403**; correct
  pair → **200**. Database inspected: secrets are opaque bytes (no plaintext private key).

## 3. Switching the TLS provider to `ring`

Reason: the default `aws_lc_rs` provider requires a C toolchain (`cmake`/`nasm`) on the
Windows target.

- `rustls` and `tokio-rustls` moved to `default-features = false` + the `ring` feature.
- `axum-server` moved from `tls-rustls` to `tls-rustls-no-provider` (so it does not force
  `aws_lc_rs`).
- `tls::install_crypto_provider` now installs `rustls::crypto::ring::default_provider`.
- `reqwest` (`rustls-tls`) and `rcgen` already used `ring`.

Result: `cargo tree` shows no `aws-lc`; roundtrip and `403` re-validated with the new
backend.

## 4. Building the server (Linux) and the client (Windows)

- Server: `cargo build --release -p server` → ELF 64-bit Linux.
- Client: since the environment had neither `rustup` nor a Windows `std`, we installed
  `rustup` **1.95.0** (home only, `--no-modify-path`), added the `x86_64-pc-windows-gnu`
  target, and configured the mingw-w64 linker in `.cargo/config.toml`.
- `cargo build --release -p client --target x86_64-pc-windows-gnu` → `client.exe` **PE32+
  x86-64** (`ring` compiled with mingw).
- We also demonstrated `new-build --target x86_64-pc-windows-gnu`, producing a
  **functional** `client.exe` with the build id, URL and 2 certificate blocks (client + CA)
  embedded.

## Current state

- `common`, `server`, `client` compile clean (debug and release).
- The server runs on Linux; the client cross-compiles to Windows without a C toolchain.
- Documentation in `docs/` (Portuguese) and `docs/en/` (English), plus the overview in
  `README.md`.
