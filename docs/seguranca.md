# Modelo de segurança e banco de dados

## Princípios

1. **Uma chave PGP por build.** As duas metades (pública e privada) são geradas no
   servidor. A privada e a passphrase são **lacradas** (cifradas) antes de irem para o
   banco e **nunca são gravadas em texto puro**.
2. **Descriptografia só no cliente.** O servidor entrega o artefato cifrado e a chave
   privada; a junção (decrypt) acontece na máquina do cliente, em memória.
3. **Entrega da chave privada é dupla-condicionada:** só por **mTLS** e só para o cliente
   cujo **CN do certificado == build id**.
4. **CA lacrada.** A chave privada da CA é cifrada com a mesma chave-mestra do banco.
5. **Perda da senha-mestra = banco irrecuperável**, por design.

## Criptografia at-rest

```
SERVER_MASTER_PASSPHRASE ──(Argon2id, salt do banco)──▶ chave-mestra (32 bytes)
                                                          │
segredo (chave PGP priv, passphrase, chave da CA) ──(XChaCha20-Poly1305, nonce 24B)──▶ blob
```

- **Derivação**: `argon2` (Argon2id) em `common/src/sealed.rs::derive_master_key`. O `salt`
  (16 bytes aleatórios) é gerado uma vez e guardado em `meta.kdf_salt`.
- **Cifra autenticada**: `chacha20poly1305::XChaCha20Poly1305`. Cada segredo tem seu próprio
  `nonce` aleatório de 24 bytes. `open` falha (autenticação) se a senha-mestra estiver
  errada ou o dado tiver sido adulterado.
- **Higiene**: buffers temporários de texto puro passam por `zeroize`.

## Onde a chave privada aparece em texto puro

- **Servidor**: apenas na RAM, no momento de responder `GET /builds/:id/key`, após validar
  o mTLS e o CN. Nunca é gravada em disco descifrada.
- **Cliente**: em memória durante o `decrypt`. A saída é o arquivo `--out` que o próprio
  usuário pediu.

## Considerações / trade-offs

- Como o servidor compila o cliente e **embute a identidade mTLS** (cert + chave do
  cliente), **possuir o binário do cliente é a identidade**. Isso é inerente ao modelo
  "servidor compila o cliente por build". Uma variante mais forte entregaria o cert do
  cliente como arquivo separado (sidecar) em vez de embutir.
- A transferência (artefato e chave) é protegida por TLS 1.2/1.3 com o provider **`ring`**.
- O artefato trafega **sempre cifrado com PGP**, independentemente do TLS.

## Esquema do banco (SQLite)

Arquivo padrão: `data/orchestrator.db` (configurável com `--db`).

### Tabela `meta` (uma linha, `id = 1`)

| Coluna | Tipo | Conteúdo |
|---|---|---|
| `id` | INTEGER PK (=1) | sentinela |
| `kdf_salt` | BLOB | salt do Argon2id |
| `ca_cert_pem` | TEXT | certificado da CA (público) |
| `ca_key_sealed` | BLOB | chave privada da CA, **lacrada** |
| `ca_key_nonce` | BLOB | nonce do lacre da chave da CA |

### Tabela `builds`

| Coluna | Tipo | Conteúdo |
|---|---|---|
| `id` | TEXT PK | build id (`build-<hex>`) |
| `created_at` | TEXT | timestamp RFC 3339 |
| `status` | TEXT | estado do build (`built`) |
| `artifact_path` | TEXT | caminho do blob PGP cifrado |
| `pub_armored` | TEXT | chave pública PGP do build |
| `priv_sealed` | BLOB | chave privada PGP, **lacrada** |
| `priv_nonce` | BLOB | nonce da chave privada |
| `passphrase_sealed` | BLOB | passphrase da chave PGP, **lacrada** |
| `passphrase_nonce` | BLOB | nonce da passphrase |
| `client_cn` | TEXT | CN esperado do certificado do cliente (= build id) |

> **Não é PostgreSQL.** O sistema usa exclusivamente SQLite embutido em arquivo local; não
> há conexão com nenhum servidor de banco externo.

### Como inspecionar

```bash
sqlite3 data/orchestrator.db "SELECT id, created_at, status, client_cn FROM builds;"
# As colunas *_sealed são bytes opacos; não há chave privada em texto puro no arquivo.
```
