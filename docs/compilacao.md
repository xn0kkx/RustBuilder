# Compilação

O **servidor** é um binário **Linux**. O **cliente** tem como alvo **Windows** e é
cross-compilado a partir do Linux com o alvo `x86_64-pc-windows-gnu`.

A stack TLS usa `rustls` com o provider **`ring`** (Rust puro). Não há `aws-lc-sys` na
árvore de dependências, então o cliente Windows linka **sem MSVC, sem `cmake`, sem `nasm`**
— basta a toolchain Rust e o linker do mingw-w64.

## Pré-requisitos

- Rust 1.95+ (funciona tanto com o pacote da distro quanto com `rustup`).
- Para o cliente Windows:
  - alvo `x86_64-pc-windows-gnu` (requer `rustup`);
  - `mingw-w64` (fornece `x86_64-w64-mingw32-gcc`).

```bash
# mingw-w64 (Debian/Kali/Ubuntu)
sudo apt install mingw-w64

# rustup + alvo Windows (caso ainda não tenha)
curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain 1.95.0
export PATH="$HOME/.cargo/bin:$PATH"
rustup target add x86_64-pc-windows-gnu
```

> O linker do alvo Windows já está configurado no repositório em `.cargo/config.toml`:
> ```toml
> [target.x86_64-pc-windows-gnu]
> linker = "x86_64-w64-mingw32-gcc"
> ar = "x86_64-w64-mingw32-ar"
> ```

## Servidor (Linux)

```bash
cargo build --release -p server
# saída: target/release/server   (ELF 64-bit Linux)
```

## Cliente (Windows)

```bash
cargo build --release -p client --target x86_64-pc-windows-gnu
# saída: target/x86_64-pc-windows-gnu/release/client.exe   (PE32+ x86-64)
```

Esse `client.exe` é o **template**: compilado sem as variáveis `OMC_*`, ele embute
valores de desenvolvimento (CA/identidade vazias). O `client.exe` **funcional** de cada
release é produzido pelo orquestrador (ver abaixo e em [execucao.md](execucao.md)).

## Compilar o cliente funcional (via orquestrador)

O subcomando `new-build` embute, em tempo de compilação, os dados específicos do build
(build id, URL, CA, identidade mTLS) e chama o `cargo` para gerar o `.exe`:

```bash
export PATH="$HOME/.cargo/bin:$PATH"   # garante que o cargo com o alvo Windows esteja no PATH
SERVER_MASTER_PASSPHRASE=sua-senha ./target/release/server \
  new-build --artifact ./payload.bin --uid release-1 \
  --server-url https://seu-host:8443 \
  --target x86_64-pc-windows-gnu
# saída: data/clients/<build-id>/client.exe
```

## Workspace completo

```bash
cargo build --release --workspace   # compila common + server + client (alvo nativo Linux)
```

## Notas de dependências (ambiente offline/registry restrito)

- `futures-util` está fixado em `=0.3.31` no `crates/server/Cargo.toml` para casar com a
  versão disponível no índice usado.
- Trocamos o provider TLS de `aws_lc_rs` para `ring` desativando as features padrão de
  `rustls`, `tokio-rustls` e usando `axum-server` com `tls-rustls-no-provider`. Confirme com:
  ```bash
  cargo tree -e no-dev | grep -i aws-lc    # não deve retornar nada
  ```
- Compressão gzip do download: servidor usa `tower-http` (feature `compression-gzip`) +
  `tokio-util` (feature `io`, para o streaming do arquivo); cliente usa a feature `gzip` do
  `reqwest`. O backend gzip é puro-Rust (`flate2`/`miniz_oxide`), **sem dependência C** — o
  cliente Windows continua linkando sem MSVC/cmake/nasm. Confirme:
  ```bash
  cargo tree | grep -i miniz_oxide                       # deve aparecer
  cargo tree | grep -iE 'zlib-ng|libz-sys'               # não deve retornar nada (backend C)
  ```
