# Troubleshooting

Common symptoms, likely cause, and how to fix. Quoted messages are the ones emitted by the
code itself.

## Server cannot open the database

**Symptom**: `"failed to unseal ca key (wrong master passphrase?)"` or
`"authentication failed while opening sealed data"` at startup.

- **Cause**: `SERVER_MASTER_PASSPHRASE` differs from the one used when the database was
  created. The master key is derived from the passphrase; a wrong passphrase cannot open
  the sealed secrets.
- **Fix**: use exactly the same passphrase every time. There is no recovery — if the
  passphrase is lost, the database is unrecoverable by design (delete
  `data/orchestrator.db` and start over, losing existing builds).

## The server seems to "hang" waiting for something

**Symptom**: the process is unresponsive after starting.

- **Cause**: `SERVER_MASTER_PASSPHRASE` is not set and the server is waiting for the
  passphrase at the interactive (hidden) prompt.
- **Fix**: run in an interactive terminal and type the passphrase, or set the environment
  variable beforehand (ideal for background/services):
  ```bash
  export SERVER_MASTER_PASSPHRASE=your-secret
  ```

## Client fails server certificate verification

**Symptom**: on the client, a TLS *certificate/hostname* error; the connection does not
complete.

- **Cause**: the URL host (`--server-url` / `--server`) is not among the server
  certificate's SANs.
- **Fix**: issue the server with the matching SAN and use the same URL:
  ```bash
  server serve --addr 0.0.0.0:8443 --san your-host --san 127.0.0.1
  # client must use https://your-host:8443
  ```
  Remember: the SAN must match the **name/IP** used in the URL, not the listen address.

## `403 Forbidden` when fetching `/key` or `/artifact`

**Symptom**: the client passes TLS but gets 403.

- **Cause**: the client certificate CN ≠ the route's build id. That is, you are using one
  build's `client.exe` to fetch another build's key.
- **Fix**: use the `client.exe` generated for that specific build (`data/clients/<id>/`),
  or pass the correct `--build-id` (which must match the embedded certificate).

## TLS handshake refused / connection drops before any response

**Symptom**: `curl -k` returns code `000`; the client gets no HTTP response.

- **Cause**: the routes require **mTLS** — without a valid client certificate the handshake
  is rejected. This is expected.
- **Fix**: present the client certificate. With `curl`:
  ```bash
  curl --cacert data/staging/<id>/ca.pem \
       --cert data/staging/<id>/identity.pem \
       --key  data/staging/<id>/identity.pem \
       https://127.0.0.1:8443/builds/<id>/key
  ```

## Client aborts with an embedded-identity error

**Symptom**: `"failed to load embedded client identity"` or
`"failed to load embedded ca certificate"`.

- **Cause**: the `client.exe` was compiled **without** the `OMC_*` variables (it is the dev
  *template*, with empty CA/identity) and is being run as if it were functional.
- **Fix**: use the `client.exe` produced by `new-build` (which embeds the build assets).
  The binary in `target/.../release/client.exe` is only for checking compilation.

## `new-build` fails to compile the client

**Symptom**: `"client build failed with status ..."` or
`"expected client binary not found at ..."`.

- **Cause 1**: the Windows target is not installed for the `cargo` the server invokes.
  - **Fix**: ensure the rustup `cargo` (with the target) is on `PATH` when running the
    server:
    ```bash
    export PATH="$HOME/.cargo/bin:$PATH"
    rustup target add x86_64-pc-windows-gnu
    ```
- **Cause 2**: the mingw linker is missing (`x86_64-w64-mingw32-gcc` not found).
  - **Fix**: `sudo apt install mingw-w64` and check `.cargo/config.toml`.
- **Cause 3**: the server runs outside the workspace root and cannot find `cargo`/the
  `client` crate.
  - **Fix**: run `new-build` from the repository root (or keep the workspace layout intact —
    the orchestrator goes two levels up from `crates/server`).

## `error[linker] x86_64-w64-mingw32-gcc: No such file`

- **Cause**: mingw-w64 not installed when cross-compiling the client.
- **Fix**: `sudo apt install mingw-w64`.

## `aws-lc-sys` starts compiling again (you wanted only `ring`)

**Symptom**: the build pulls `aws-lc-sys` and demands `cmake`/`nasm`.

- **Cause**: some dependency re-enabled the `rustls` `aws_lc_rs` feature (features unify
  across the workspace).
- **Fix**: keep `rustls`/`tokio-rustls` with `default-features = false` + `ring` and
  `axum-server` with `tls-rustls-no-provider`. Verify:
  ```bash
  cargo tree -e no-dev | grep -i aws-lc   # should be empty
  ```

## `failed to select a version for the requirement futures-macro = "=0.3.34"`

- **Cause**: a crates index without `futures-util 0.3.34` (which pins `futures-macro
  0.3.34`).
- **Fix**: already handled — `crates/server/Cargo.toml` pins `futures-util = "=0.3.31"`.
  Adjust the version to whatever your index offers.

## `Address already in use` on `serve`

- **Cause**: the port (default `8443`) is already taken, usually by a previous server.
- **Fix**: stop the old process or use another port:
  ```bash
  ss -ltnp | grep 8443
  server serve --addr 0.0.0.0:9443 --san your-host
  ```

## Connection refused on the client

- **Cause**: the server is not running, or the URL/port are wrong.
- **Fix**: confirm `serve` is running and that `--server`/`--server-url` point to the
  correct host:port.

## `rustup: command not found`

- **Cause**: the distro Rust does not manage extra targets and there is no `rustup`.
- **Fix**: install `rustup` (home only, without changing the shell):
  ```bash
  curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain 1.95.0
  export PATH="$HOME/.cargo/bin:$PATH"
  ```
