# Documentação — RustBuilder

Sistema **servidor/cliente** em Rust onde o servidor gera uma **chave PGP nova a cada
build**, criptografa o artefato com essa chave, guarda todos os segredos num **banco
SQLite criptografado** e **compila um binário cliente dedicado** para aquele build. O
cliente baixa o artefato criptografado por **mTLS**, busca a chave privada no servidor e
**descriptografa localmente** — o texto puro nunca existe no servidor em tempo de
execução.

- **Servidor**: roda em **Linux**.
- **Cliente**: alvo **Windows** (`x86_64-pc-windows-gnu`), cross-compilado a partir do Linux.

## Índice

| Documento | Conteúdo |
|---|---|
| [arquitetura.md](arquitetura.md) | Componentes, fluxo ponta a ponta, crates |
| [seguranca.md](seguranca.md) | Modelo de segurança, criptografia at-rest, banco de dados |
| [api.md](api.md) | Endpoints HTTP/mTLS |
| [antivm.md](antivm.md) | Proteção anti-VM opcional (feature `antivm`) e cópia local corrigida |
| [compilacao.md](compilacao.md) | Como compilar o servidor (Linux) e o cliente (Windows) |
| [execucao.md](execucao.md) | Como executar cada binário e o fluxo completo |
| [troubleshooting.md](troubleshooting.md) | Erros comuns e como resolver |
| [historico.md](historico.md) | O que foi feito, em ordem, e as decisões tomadas |

> **English documentation**: [`en/README.md`](en/README.md).

## Início rápido

```bash
# 1. Servidor (Linux)
cargo build --release -p server

# 2. Cliente (Windows) — pré-requisitos em compilacao.md
cargo build --release -p client --target x86_64-pc-windows-gnu

# 3. Criar um build (gera chave, criptografa, compila cliente Windows)
SERVER_MASTER_PASSPHRASE=... ./target/release/server \
  new-build --artifact ./payload.bin --uid release-1 \
  --server-url https://seu-host:8443 --target x86_64-pc-windows-gnu

# 4. Servir (mTLS)
SERVER_MASTER_PASSPHRASE=... ./target/release/server \
  serve --addr 0.0.0.0:8443 --san seu-host

# 5. No Windows, rodar o client.exe gerado
client.exe --out C:\saida\arquivo.bin
```

## Stack

- **HTTP/mTLS**: `axum` + `axum-server` + `rustls` (provider **`ring`**, Rust puro).
- **PGP**: `pgp` (rpgp), Rust puro.
- **Banco**: `rusqlite` (SQLite embutido, feature `bundled`).
- **Criptografia at-rest**: `argon2` (Argon2id) + `chacha20poly1305` (XChaCha20-Poly1305).
- **Certificados**: `rcgen` (CA + certificados de servidor e cliente).
