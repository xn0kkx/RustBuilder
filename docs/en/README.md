# Documentation — RustBuilder

A **server/client** system in Rust where the server generates a **fresh PGP key per
build**, encrypts the artifact with that key, stores every secret in an **encrypted SQLite
database**, and **compiles a dedicated client binary** for that build. The client
downloads the encrypted artifact over **mTLS**, fetches the private key from the server,
and **decrypts locally** — the plaintext never exists on the server at runtime.

- **Server**: runs on **Linux**.
- **Client**: targets **Windows** (`x86_64-pc-windows-gnu`), cross-compiled from Linux.

> Documentação em português: [`../README.md`](../README.md).

## Index

| Document | Contents |
|---|---|
| [architecture.md](architecture.md) | Components, end-to-end flow, crates |
| [security.md](security.md) | Security model, at-rest crypto, database |
| [api.md](api.md) | HTTP/mTLS endpoints |
| [antivm.md](antivm.md) | Optional anti-VM protection (`antivm` feature) and patched local copy |
| [build.md](build.md) | How to compile the server (Linux) and client (Windows) |
| [run.md](run.md) | How to run each binary and the full flow |
| [history.md](history.md) | What was done, in order, and the decisions made |
| [troubleshooting.md](troubleshooting.md) | Common errors and fixes |

## Current status

The implemented flow includes per-build PGP keys, Argon2id/XChaCha20-Poly1305
sealed storage, an mTLS-protected API, gzip-capable streaming downloads,
client-encrypted diagnostic uploads, and Linux-to-Windows client cross-compilation.
Anti-VM protection is enabled by default and can be disabled with `--no-antivm`.

Encrypted diagnostics are currently persisted as opaque `.pgp` blobs under
`data/diagnostics/<build-id>/`; SQLite indexing remains planned.

## Quick start

```bash
# 1. Server (Linux)
cargo build --release -p server

# 2. Client (Windows) — prerequisites in build.md
cargo build --release -p client --target x86_64-pc-windows-gnu

# Alternative: build.sh builds the release server and Windows client
./build.sh

# 3. Create a build (generates key, encrypts, compiles the Windows client)
SERVER_MASTER_PASSPHRASE=... ./target/release/server \
  new-build --artifact ./payload.bin --uid release-1 \
  --server-url https://your-host:8443 --target x86_64-pc-windows-gnu

# 4. Serve (mTLS)
SERVER_MASTER_PASSPHRASE=... ./target/release/server \
  serve --addr 0.0.0.0:8443 --san your-host

# 5. On Windows, run the generated client.exe
client.exe --out C:\out\file.bin
```

## Stack

- **HTTP/mTLS**: `axum` + `axum-server` + `rustls` (**`ring`** provider, pure Rust).
- **PGP**: `pgp` (rpgp), pure Rust.
- **Database**: `rusqlite` (embedded SQLite, `bundled` feature).
- **At-rest crypto**: `argon2` (Argon2id) + `chacha20poly1305` (XChaCha20-Poly1305).
- **Certificates**: `rcgen` (CA + server and client certificates).
