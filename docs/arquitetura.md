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
                             b. GET do artefato (gzip no transporte) baixado em chunks
                                para um arquivo temp cifrado <out>.part no disco
                             c. GET da chave privada do build
                             d. descriptografa o temp em memória, grava o texto puro
                                localmente, remove o <out>.part e executa o artefato
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
      src/main.rs            # CLI, console, rotas axum, binding do CN mTLS
      src/db.rs              # schema rusqlite + colunas lacradas
      src/ca.rs              # CA rcgen + emissão de certificados
      src/orchestrator.rs    # gera chave, criptografa artefato, invoca cargo build
      src/tls.rs             # ServerConfig rustls com verificação de cliente (mTLS)
    client/                  # baixador por build (binário Windows)
      build.rs               # embute os assets do build via variáveis de ambiente
      src/main.rs            # reqwest mTLS, download/decrypt/execução, upload de diagnóstico
      src/utils/             # execução do artefato e utilitários opcionais de obfuscação
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
Binário Linux com comandos de build, serviço, administração e console. Detalhes em
[execucao.md](execucao.md) e [api.md](api.md).

- **`orchestrator.rs`** — `new_build`:
  1. gera `build_id` (`build-` + hex de SHA-256(uid + nanos)) e uma passphrase aleatória;
  2. `generate_keypair` → lacra chave privada + passphrase → insere a linha do build;
  3. `encrypt_to_public(artefato)` → grava o blob cifrado em `data/artifacts/<id>.pgp`;
  4. emite o certificado de cliente (CN = build_id) assinado pela CA;
  5. compila o cliente: `cargo build -p client --release [--target ...]` com as variáveis
    `OMC_BUILD_ID`, `OMC_SERVER_URL`, `OMC_CA_CERT`, `OMC_CLIENT_IDENTITY`,
    `OMC_BUILD_PUBKEY` e `OMC_DEBUG_CLIENT`, e copia o
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
  um `config.rs` com `BUILD_ID`, `SERVER_URL` e `include_bytes!` do cert da CA, da
  identidade mTLS (chave + cert concatenados) e da **chave pública PGP do build** (`PUB_KEY`,
  via `OMC_BUILD_PUBKEY`). Sem as variáveis, usa defaults de dev, então o crate ainda compila
  de forma avulsa.
- **`main.rs`** — monta um `reqwest::blocking::Client` com `use_rustls_tls()` (feature
  `gzip`: negocia `Accept-Encoding: gzip` e descomprime de forma transparente), adiciona a
  CA embutida como raiz e a identidade mTLS embutida; `download_artifact` baixa o artefato em
  chunks (buffer de 64 KiB) gravando o ciphertext num arquivo temporário `<out>.part`; faz
  `GET` da chave; lê o temp, chama `common::pgp::decrypt`, grava `--out` e remove o temp.
  Como o servidor envia o artefato via `Body::from_stream` (leitura do disco em chunks) e o
  `CompressionLayer` comprime em streaming, nem o servidor nem o cliente carregam o artefato
  inteiro além do necessário para a descriptografia em memória.
  - **Upload de diagnósticos (cliente → servidor).** A função reutilizável `upload_diagnostic`
    cifra um arquivo de diagnóstico com a `PUB_KEY` embutida (`common::pgp::encrypt_to_public`),
    grava o ciphertext num temp `<file>.part` e faz `POST /builds/:id/diagnostics` com o corpo
    em streaming (chunked). O servidor grava o blob cifrado em `data/diagnostics/<id>/` em
    chunks (sem carga total na RAM). Reusa o par de chaves do build: o servidor pode abrir o
    blob depois com a privada lacrada + passphrase. É o caminho inverso do download —
    criptografia no cliente, blob opaco no servidor.
- **`utils::exec`** — no Windows, copia os bytes baixados para memória executável em chunks
  de 256 bytes e chama o entry point; builds não-Windows rejeitam execução de shellcode.
- **`utils::obf`** — com a feature `obfs`, oferece o subcomando `obf` para representações
  IPv4, IPv6, MAC e UUID.
- **Proteção anti-VM (opcional).** A feature `antivm` (ligada por padrão) embute a biblioteca
    `antivm` e chama a proteção no início do `main`, com efeito apenas no alvo Windows
    (`#[cfg(all(windows, feature = "antivm"))]`). Desligável com `--no-default-features` (ou
    `new-build --no-antivm`) para testes de desenvolvimento. Detalhes em [antivm.md](antivm.md).

## Fluxo de confiança (mTLS)

- Uma **CA** única (criada no primeiro uso, chave privada lacrada no banco) assina tanto o
  certificado do **servidor** quanto os certificados de **cliente** (um por build).
- O servidor **exige** certificado de cliente válido encadeado à CA (transporte
  autenticado) e, além disso, verifica que o **CN do certificado do cliente == build id**
  da rota. Assim, o cliente do build A não consegue buscar a chave do build B.
