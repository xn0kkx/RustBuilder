# HTTP / mTLS API

All routes require **mTLS**: the client must present a valid certificate chaining to the
server's CA. Without a client certificate, the TLS handshake fails before any route is
reached.

Base URL: `https://<host>:<port>` (default `https://127.0.0.1:8443`).

## `GET /builds/:id/artifact`

Returns the **PGP-encrypted** artifact (opaque bytes).

- **Authorization**: mTLS + the client certificate CN must equal `:id`.
- **Responses**:
  - `200 OK` — `application/octet-stream` body with the PGP ciphertext.
  - `403 Forbidden` — certificate CN ≠ `:id`.
  - `404 Not Found` — build does not exist.
  - `500` — error reading the blob.

## `GET /builds/:id/key`

Returns the build's PGP private key and passphrase (for local decryption).

- **Authorization**: mTLS + certificate CN == `:id` (checked twice: in `Extension<PeerCn>`
  and against `builds.client_cn`).
- **`200 OK` response** (JSON):

```json
{
  "build_id": "build-4789a71d0f101278",
  "priv_armored": "-----BEGIN PGP PRIVATE KEY BLOCK-----\n...",
  "passphrase": "…"
}
```

- **Errors**: `403` (CN mismatch), `404` (build does not exist), `500` (failure opening the
  sealed secrets).

## `POST /builds/:id/diagnostics`

Receives a **diagnostic file** uploaded by the client, **PGP-encrypted** (with the build's
public key, embedded in the client). The server just **persists the encrypted blob** to disk
(`data/diagnostics/<id>/<timestamp>-<name>.pgp`); it will be stored in the database in the
future. No plaintext travels or is written on the server.

- **Authorization**: mTLS + the client certificate CN must equal `:id` (same cert → build
  binding as the other routes).
- **Body**: `application/octet-stream` with the PGP ciphertext, sent as a **stream**
  (`Transfer-Encoding: chunked`) — the server writes it in chunks, without loading it all into
  RAM (symmetric to the download streaming).
- **Optional header** `X-Diagnostic-Filename`: original file name. The server uses only the
  *basename* (sanitized — no path components); defaults to `diagnostic` if absent.
- **Responses**:
  - `200 OK` (JSON):

    ```json
    {
      "build_id": "build-4789a71d0f101278",
      "filename": "1725800000000000000-diag.txt.pgp",
      "bytes": 512
    }
    ```
  - `403 Forbidden` — certificate CN ≠ `:id`.
  - `404 Not Found` — build does not exist.
  - `400 Bad Request` — failure reading the body; `500` — failure writing to disk.

## Quick check with `curl`

A build's `ca.pem` and `identity.pem` files live in `data/staging/<id>/` (generated during
`new-build`).

```bash
CA=data/staging/<id>/ca.pem
IDENT=data/staging/<id>/identity.pem   # contains the client key + cert

# success (correct cert and id)
curl --cacert "$CA" --cert "$IDENT" --key "$IDENT" \
  https://127.0.0.1:8443/builds/<id>/key

# 403: valid cert, but wrong build id
curl -o /dev/null -w "%{http_code}\n" --cacert "$CA" --cert "$IDENT" --key "$IDENT" \
  https://127.0.0.1:8443/builds/other-id/key

# refused handshake: no client certificate
curl -k https://127.0.0.1:8443/builds/<id>/key
```

Expected, already validated behavior: **200** for the correct pair, **403** for the wrong
id, and a refused handshake without a certificate.
