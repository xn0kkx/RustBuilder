# API HTTP / mTLS

Todas as rotas exigem **mTLS**: o cliente precisa apresentar um certificado válido
encadeado à CA do servidor. Sem certificado de cliente, o handshake TLS falha antes de
qualquer rota ser atingida.

Base URL: `https://<host>:<porta>` (padrão `https://127.0.0.1:8443`).

## Compressão e streaming

Todas as respostas passam por um `CompressionLayer` (gzip). Quando o cliente envia
`Accept-Encoding: gzip`, o servidor responde com `Content-Encoding: gzip` e
`Transfer-Encoding: chunked` (o corpo é comprimido em streaming; respostas pequenas abaixo do
limiar do layer não são comprimidas). O corpo, uma vez decodificado o gzip, é exatamente o
mesmo conteúdo descrito abaixo — o `reqwest` do cliente (feature `gzip`) descomprime de forma
transparente. O artefato é lido do disco e enviado em chunks, sem carga total em memória.

## `GET /builds/:id/artifact`

Retorna o artefato **criptografado com PGP** (bytes opacos).

- **Autorização**: mTLS + o CN do certificado do cliente deve ser igual a `:id`.
- **Respostas**:
  - `200 OK` — corpo `application/octet-stream` com o ciphertext PGP (após decodificar o
    gzip do transporte, se negociado).
  - `403 Forbidden` — CN do certificado ≠ `:id`.
  - `404 Not Found` — build inexistente.
  - `500` — erro ao abrir o blob.

## `GET /builds/:id/key`

Retorna a chave privada PGP do build e a passphrase (para descriptografia local).

- **Autorização**: mTLS + CN do certificado == `:id` (verificado duas vezes: no
  `Extension<PeerCn>` e contra `builds.client_cn`).
- **Resposta `200 OK`** (JSON):

```json
{
  "build_id": "build-4789a71d0f101278",
  "priv_armored": "-----BEGIN PGP PRIVATE KEY BLOCK-----\n...",
  "passphrase": "…"
}
```

- **Erros**: `403` (CN divergente), `404` (build inexistente), `500` (falha ao abrir os
  segredos lacrados).

## `POST /builds/:id/diagnostics`

Recebe um **arquivo de diagnóstico** enviado pelo cliente, **criptografado com PGP** (a chave
pública do build, embutida no cliente). O servidor apenas **persiste o blob cifrado** em disco
(`data/diagnostics/<id>/<timestamp>-<nome>.pgp`); futuramente será salvo no banco. Nenhum texto
puro trafega nem é gravado no servidor.

- **Autorização**: mTLS + o CN do certificado do cliente deve ser igual a `:id` (mesma
  amarração cert → build das demais rotas).
- **Corpo**: `application/octet-stream` com o ciphertext PGP, enviado em **streaming**
  (`Transfer-Encoding: chunked`) — o servidor grava em chunks, sem carregar tudo na RAM
  (simétrico ao streaming do download).
- **Header opcional** `X-Diagnostic-Filename`: nome original do arquivo. O servidor usa apenas
  o *basename* (sanitizado — sem componentes de caminho); default `diagnostic` se ausente.
- **Respostas**:
  - `200 OK` (JSON):

    ```json
    {
      "build_id": "build-4789a71d0f101278",
      "filename": "1725800000000000000-diag.txt.pgp",
      "bytes": 512
    }
    ```
  - `403 Forbidden` — CN do certificado ≠ `:id`.
  - `404 Not Found` — build inexistente.
  - `400 Bad Request` — falha ao ler o corpo; `500` — falha ao gravar em disco.

## Verificação rápida com `curl`

Os arquivos `ca.pem` e `identity.pem` de um build ficam em `data/staging/<id>/` (gerados
no `new-build`).

```bash
CA=data/staging/<id>/ca.pem
IDENT=data/staging/<id>/identity.pem   # contém chave + cert do cliente

# sucesso (cert e id corretos)
curl --cacert "$CA" --cert "$IDENT" --key "$IDENT" \
  https://127.0.0.1:8443/builds/<id>/key

# 403: cert válido, mas id de build errado
curl -o /dev/null -w "%{http_code}\n" --cacert "$CA" --cert "$IDENT" --key "$IDENT" \
  https://127.0.0.1:8443/builds/outro-id/key

# handshake recusado: sem certificado de cliente
curl -k https://127.0.0.1:8443/builds/<id>/key
```

Comportamento esperado, já validado: **200** no par correto, **403** no id errado,
handshake recusado sem certificado.
