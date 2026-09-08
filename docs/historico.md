# Histórico do que foi feito

Ordem cronológica das etapas de desenvolvimento e das decisões tomadas.

## 1. Baixador PGP inicial (crate única)

Programa Rust que baixa um arquivo por HTTPS e o descriptografa **localmente** com uma
chave PGP. Base do `common::pgp::decrypt` atual.

- HTTP: `reqwest` (blocking) com `rustls-tls`.
- PGP: `pgp` (rpgp), Rust puro (evita dependências C do sequoia).
- Aceita mensagens ASCII-armored e binárias (sniff do prefixo `-----BEGIN PGP`).
- Validado ponta a ponta: chave GPG descartável → arquivo cifrado → download → decrypt →
  `diff` idêntico.

## 2. Evolução para servidor + cliente (orquestração de builds)

Requisito: a cada novo build, uma **chave de criptografia diferente** salva num **banco
criptografado**; o servidor **orquestra builds e conexões**.

Decisões acordadas:
- "build" = o servidor **compila o binário do cliente** por release, cada um com sua chave.
- O servidor **gera o par PGP**, guarda as duas metades lacradas e **entrega a chave
  privada ao cliente autenticado sob demanda**.
- Stack: **Rust + axum + SQLite com criptografia em camada de aplicação** (Argon2id +
  XChaCha20-Poly1305).
- Autenticação cliente↔servidor: **mTLS** (CA do servidor emite um cert por build).

Implementação:
- Workspace com três crates: `common`, `server`, `client`.
- `common`: `generate_keypair`, `encrypt_to_public`, `decrypt`; lacre `seal`/`open`.
- `server`: CA (`rcgen`), banco (`rusqlite` bundled), orquestrador (compila o cliente),
  TLS mútuo (`rustls` + `WebPkiClientVerifier` + acceptor customizado que lê o CN do
  certificado do cliente), rotas `/builds/:id/artifact` e `/builds/:id/key`.
- `client`: `build.rs` embute build id, URL, CA e identidade mTLS; `main.rs` faz o fetch
  por mTLS e descriptografa localmente.

Validações:
- Build e release limpos, **zero warnings**.
- Roundtrip do PGP (keygen → encrypt → decrypt) confirmado por teste temporário.
- Ponta a ponta: `new-build` → `serve` → cliente → `diff` **idêntico**.
- Segurança: sem cert de cliente → handshake recusado; cert válido em id errado → **403**;
  par correto → **200**. Banco inspecionado: segredos são bytes opacos (sem chave privada
  em texto puro).

## 3. Troca do provider TLS para `ring`

Motivo: o provider padrão `aws_lc_rs` exige toolchain C (`cmake`/`nasm`) no alvo Windows.

- `rustls` e `tokio-rustls` passaram a usar `default-features = false` + feature `ring`.
- `axum-server` passou de `tls-rustls` para `tls-rustls-no-provider` (para não forçar
  `aws_lc_rs`).
- `tls::install_crypto_provider` passou a instalar `rustls::crypto::ring::default_provider`.
- `reqwest` (`rustls-tls`) e `rcgen` já usavam `ring`.

Resultado: `cargo tree` sem nenhum `aws-lc`; roundtrip e `403` revalidados com o novo
backend.

## 4. Compilação do servidor (Linux) e do cliente (Windows)

- Servidor: `cargo build --release -p server` → ELF 64-bit Linux.
- Cliente: como o ambiente não tinha `rustup` nem `std` de Windows, instalamos o `rustup`
  **1.95.0** (só no `$HOME`, `--no-modify-path`), adicionamos o alvo
  `x86_64-pc-windows-gnu` e configuramos o linker mingw-w64 em `.cargo/config.toml`.
- `cargo build --release -p client --target x86_64-pc-windows-gnu` → `client.exe` **PE32+
  x86-64** (o `ring` compilou com o mingw).
- Demonstramos também o `new-build --target x86_64-pc-windows-gnu`, gerando um `client.exe`
  **funcional** com build id, URL e 2 blocos de certificado (cliente + CA) embutidos.

## Estado atual

- `common`, `server`, `client` compilam limpos (dev e release).
- Servidor roda em Linux; cliente cross-compila para Windows sem toolchain C.
- Documentação em `docs/` e visão geral em `README.md`.
