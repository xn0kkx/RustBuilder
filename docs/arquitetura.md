# Arquitetura

## Visão geral

```
requisição de novo build ──▶ SERVIDOR (Linux)
                             1. gera par de chaves PGP (rpgp)
                             2. gera certificado de cliente por build, assinado pela CA (rcgen)
                             3. criptografa o artefato com a chave pública  ──▶ blob cifrado
                             4. lacra chave privada + passphrase (XChaCha20-Poly1305) ──▶ SQLite
                             5. compila o binário do cliente (cargo --target windows-gnu)
                                embutindo: build id, URL do servidor, cert da CA, cert+chave do cliente
                                   │
                                   ▼
                             CLIENT.EXE (Windows, autocontido, por build)
                             a. conecta via mTLS (apresenta o cert embutido)
                             b. GET do artefato criptografado
                             c. GET da chave privada do build
                             d. descriptografa em memória e grava o texto puro localmente
```

O texto puro (plaintext) só existe **na máquina do cliente**: em memória durante a
descriptografia e no arquivo de saída solicitado.

## Crates (workspace Cargo)

```
RustBuilder/
  Cargo.toml                 # [workspace] members = crates/*
  .cargo/config.toml         # linker mingw para o alvo windows-gnu
  crates/
    common/                  # biblioteca compartilhada
      src/pgp.rs             # geração de chave, encrypt p/ pública, decrypt
      src/sealed.rs          # chave-mestra Argon2id + seal/open XChaCha20-Poly1305
      src/proto.rs           # tipos serde de requisição/resposta
    server/                  # API + orquestrador (binário Linux)
      src/main.rs            # CLI (serve / new-build), rotas axum, binding do CN mTLS
      src/db.rs              # schema rusqlite + colunas lacradas
      src/ca.rs              # CA rcgen + emissão de certificados
      src/orchestrator.rs    # gera chave, criptografa artefato, invoca cargo build
      src/tls.rs             # ServerConfig rustls com verificação de cliente (mTLS)
    client/                  # baixador por build (binário Windows)
      build.rs               # embute os assets do build via variáveis de ambiente
      src/main.rs            # reqwest mTLS, busca chave+artefato, decrypt, grava saída
```

### `common`
Biblioteca sem estado, usada pelo servidor e pelo cliente.

- **`pgp.rs`**
  - `generate_keypair(uid, passphrase) -> (pub_armored, priv_armored)`: RSA 3072 com
    subchave de criptografia; algoritmos preferidos AES-256 / SHA-256 / ZLIB.
  - `encrypt_to_public(plaintext, pub_armored) -> Vec<u8>`: mensagem OpenPGP ASCII-armored.
  - `decrypt(ciphertext, priv_armored, passphrase) -> Vec<u8>`: aceita armored e binário.
- **`sealed.rs`**
  - `derive_master_key(passphrase, salt)`: Argon2id → chave de 32 bytes.
  - `seal` / `open`: XChaCha20-Poly1305 com nonce aleatório de 24 bytes.
  - `seal_str` / `open_str`: conveniência para strings, com `zeroize` do buffer temporário.
- **`proto.rs`**: `KeyResponse { build_id, priv_armored, passphrase }`, `BuildInfo`.

### `server`
Binário Linux com dois subcomandos (`new-build`, `serve`). Detalhes em
[execucao.md](execucao.md) e [api.md](api.md).

- **`orchestrator.rs`** — `new_build`:
  1. gera `build_id` (`build-` + hex de SHA-256(uid + nanos)) e uma passphrase aleatória;
  2. `generate_keypair` → lacra chave privada + passphrase → insere a linha do build;
  3. `encrypt_to_public(artefato)` → grava o blob cifrado em `data/artifacts/<id>.pgp`;
  4. emite o certificado de cliente (CN = build_id) assinado pela CA;
  5. compila o cliente: `cargo build -p client --release [--target ...]` com as variáveis
     `OMC_BUILD_ID`, `OMC_SERVER_URL`, `OMC_CA_CERT`, `OMC_CLIENT_IDENTITY`, e copia o
     binário para `data/clients/<id>/`.
- **`tls.rs`** — `ServerConfig` do rustls com `WebPkiClientVerifier` (exige certificado de
  cliente encadeado à CA). Um `MtlsAcceptor` customizado lê o CN do certificado do cliente
  após o handshake e injeta em `Extension<PeerCn>`, permitindo amarrar cert → build nas
  rotas.
- **`ca.rs`** — cria/carrega a CA e emite certificados de servidor (`ServerAuth`, com SANs)
  e de cliente (`ClientAuth`, CN = build_id). `common_name_from_der` extrai o CN via
  `x509-parser`.
- **`db.rs`** — schema e helpers; todas as colunas de segredo passam por `sealed`.

### `client`
Binário Windows gerado por build.

- **`build.rs`** — lê as variáveis `OMC_*` (caminhos de arquivos PEM) e gera, em `OUT_DIR`,
  um `config.rs` com `BUILD_ID`, `SERVER_URL` e `include_bytes!` do cert da CA e da
  identidade mTLS (chave + cert concatenados). Sem as variáveis, usa defaults de dev, então
  o crate ainda compila de forma avulsa.
- **`main.rs`** — monta um `reqwest::blocking::Client` com `use_rustls_tls()`, adiciona a
  CA embutida como raiz e a identidade mTLS embutida; faz `GET` do artefato e da chave;
  chama `common::pgp::decrypt`; grava o arquivo `--out`.

## Fluxo de confiança (mTLS)

- Uma **CA** única (criada no primeiro uso, chave privada lacrada no banco) assina tanto o
  certificado do **servidor** quanto os certificados de **cliente** (um por build).
- O servidor **exige** certificado de cliente válido encadeado à CA (transporte
  autenticado) e, além disso, verifica que o **CN do certificado do cliente == build id**
  da rota. Assim, o cliente do build A não consegue buscar a chave do build B.
