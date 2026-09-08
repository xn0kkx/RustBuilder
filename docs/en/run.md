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
| `--out <file>` | (required) | where to write the decrypted file |
| `--server <url>` | value embedded in the build | override the server URL |
| `--build-id <id>` | value embedded in the build | override the build id |

The client connects over mTLS (with the embedded identity), downloads the encrypted
artifact and the private key, **decrypts locally**, and writes to `--out`.

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
