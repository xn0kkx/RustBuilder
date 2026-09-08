# Security model and database

## Principles

1. **One PGP key per build.** Both halves (public and private) are generated on the server.
   The private key and passphrase are **sealed** (encrypted) before going to the database
   and are **never written in plaintext**.
2. **Decryption only on the client.** The server delivers the encrypted artifact and the
   private key; the join (decrypt) happens on the client machine, in memory.
3. **Private-key delivery is doubly gated:** only over **mTLS** and only to the client
   whose **certificate CN == the build id**.
4. **Sealed CA.** The CA private key is encrypted with the same database master key.
5. **Losing the master passphrase = unrecoverable database**, by design.

## At-rest crypto

```
SERVER_MASTER_PASSPHRASE ──(Argon2id, db salt)──▶ master key (32 bytes)
                                                   │
secret (PGP priv key, passphrase, CA key) ──(XChaCha20-Poly1305, 24B nonce)──▶ blob
```

- **Derivation**: `argon2` (Argon2id) in `common/src/sealed.rs::derive_master_key`. The
  `salt` (16 random bytes) is generated once and stored in `meta.kdf_salt`.
- **Authenticated cipher**: `chacha20poly1305::XChaCha20Poly1305`. Each secret has its own
  random 24-byte `nonce`. `open` fails (authentication) if the master passphrase is wrong
  or the data was tampered with.
- **Hygiene**: temporary plaintext buffers are `zeroize`-d.

## Where the private key appears in plaintext

- **Server**: only in RAM, at the moment of answering `GET /builds/:id/key`, after
  validating mTLS and the CN. Never written to disk decrypted.
- **Client**: in memory during `decrypt`. The output is the `--out` file the user
  requested.

## Considerations / trade-offs

- Because the server compiles the client and **embeds the mTLS identity** (client cert +
  key), **possession of the client binary is the identity**. This is inherent to the
  "server compiles the client per build" model. A stronger variant would deliver the
  client cert as a separate (sidecar) file instead of embedding it.
- The transfer (artifact and key) is protected by TLS 1.2/1.3 with the **`ring`** provider.
- The artifact travels **always PGP-encrypted**, regardless of TLS.

## Database schema (SQLite)

Default file: `data/orchestrator.db` (configurable with `--db`).

### `meta` table (single row, `id = 1`)

| Column | Type | Contents |
|---|---|---|
| `id` | INTEGER PK (=1) | sentinel |
| `kdf_salt` | BLOB | Argon2id salt |
| `ca_cert_pem` | TEXT | CA certificate (public) |
| `ca_key_sealed` | BLOB | CA private key, **sealed** |
| `ca_key_nonce` | BLOB | nonce for the CA key seal |

### `builds` table

| Column | Type | Contents |
|---|---|---|
| `id` | TEXT PK | build id (`build-<hex>`) |
| `created_at` | TEXT | RFC 3339 timestamp |
| `status` | TEXT | build state (`built`) |
| `artifact_path` | TEXT | path to the encrypted PGP blob |
| `pub_armored` | TEXT | build's PGP public key |
| `priv_sealed` | BLOB | PGP private key, **sealed** |
| `priv_nonce` | BLOB | private-key nonce |
| `passphrase_sealed` | BLOB | PGP key passphrase, **sealed** |
| `passphrase_nonce` | BLOB | passphrase nonce |
| `client_cn` | TEXT | expected client-certificate CN (= build id) |

> **Not PostgreSQL.** The system uses exclusively embedded, file-based SQLite; there is no
> connection to any external database server.

### How to inspect

```bash
sqlite3 data/orchestrator.db "SELECT id, created_at, status, client_cn FROM builds;"
# The *_sealed columns are opaque bytes; there is no plaintext private key in the file.
```
