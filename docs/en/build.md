# Building

The **server** is a **Linux** binary. The **client** targets **Windows** and is
cross-compiled from Linux with the `x86_64-pc-windows-gnu` target.

The TLS stack uses `rustls` with the **`ring`** provider (pure Rust). There is no
`aws-lc-sys` in the dependency tree, so the Windows client links **without MSVC, `cmake`,
or `nasm`** — you only need the Rust toolchain and the mingw-w64 linker.

## Prerequisites

- Rust 1.95+ (works with either the distro package or `rustup`).
- For the Windows client:
  - the `x86_64-pc-windows-gnu` target (requires `rustup`);
  - `mingw-w64` (provides `x86_64-w64-mingw32-gcc`).

```bash
# mingw-w64 (Debian/Kali/Ubuntu)
sudo apt install mingw-w64

# rustup + Windows target (if not present yet)
curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain 1.95.0
export PATH="$HOME/.cargo/bin:$PATH"
rustup target add x86_64-pc-windows-gnu
```

> The Windows target's linker is already configured in the repo at `.cargo/config.toml`:
> ```toml
> [target.x86_64-pc-windows-gnu]
> linker = "x86_64-w64-mingw32-gcc"
> ar = "x86_64-w64-mingw32-ar"
> ```

## Server (Linux)

```bash
cargo build --release -p server
# output: target/release/server   (ELF 64-bit Linux)
```

To build the release server and Windows client in one step:

```bash
./build.sh
```

Use `./build.sh --debug` for native debug builds. The script only compiles the
crates and does not modify the artifacts under `data/`.

## Client (Windows)

```bash
cargo build --release -p client --target x86_64-pc-windows-gnu
# output: target/x86_64-pc-windows-gnu/release/client.exe   (PE32+ x86-64)
```

This `client.exe` is the **template**: compiled without the `OMC_*` variables, it embeds
development values (empty CA/identity). The **functional** `client.exe` for each release is
produced by the orchestrator (see below and [run.md](run.md)).

## Building the functional client (via the orchestrator)

The `new-build` subcommand embeds, at compile time, the build-specific data (build id,
URL, CA, mTLS identity) and invokes `cargo` to produce the `.exe`:

```bash
export PATH="$HOME/.cargo/bin:$PATH"   # ensure the cargo with the Windows target is on PATH
SERVER_MASTER_PASSPHRASE=your-secret ./target/release/server \
  new-build --artifact ./payload.bin --uid release-1 \
  --server-url https://your-host:8443 \
  --target x86_64-pc-windows-gnu
# output: data/clients/<build-id>/client.exe
```

## Full workspace

```bash
cargo build --release --workspace   # builds common + server + client (native Linux target)
```

## Dependency notes (offline / restricted registry)

- `futures-util` is pinned to `=0.3.31` in `crates/server/Cargo.toml` to match the version
  available in the index used.
- We switched the TLS provider from `aws_lc_rs` to `ring` by disabling default features of
  `rustls`, `tokio-rustls`, and using `axum-server` with `tls-rustls-no-provider`. Verify:
  ```bash
  cargo tree -e no-dev | grep -i aws-lc    # should return nothing
  ```
