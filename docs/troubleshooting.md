# Troubleshooting

Sintomas comuns, causa provável e como resolver. As mensagens entre aspas são as emitidas
pelo próprio código.

## Servidor não abre o banco

**Sintoma**: `"failed to unseal ca key (wrong master passphrase?)"` ou
`"authentication failed while opening sealed data"` ao iniciar.

- **Causa**: `SERVER_MASTER_PASSPHRASE` diferente da usada quando o banco foi criado. A
  chave-mestra é derivada da senha; senha errada não abre os segredos lacrados.
- **Solução**: use exatamente a mesma senha de sempre. Não há recuperação — se a senha foi
  perdida, o banco é irrecuperável por design (apague `data/orchestrator.db` e recomece,
  perdendo os builds existentes).

## O servidor fica "travado" esperando algo

**Sintoma**: o processo não responde após iniciar.

- **Causa**: `SERVER_MASTER_PASSPHRASE` não está definida e o servidor está aguardando a
  senha no prompt interativo (entrada oculta).
- **Solução**: rode num terminal interativo e digite a senha, ou defina a variável de
  ambiente antes (ideal para background/serviços):
  ```bash
  export SERVER_MASTER_PASSPHRASE=sua-senha
  ```

## Cliente falha na verificação do certificado do servidor

**Sintoma**: no cliente, erro de TLS do tipo *certificate/hostname*; conexão não completa.

- **Causa**: o host da URL (`--server-url` / `--server`) não está entre os SANs do
  certificado do servidor.
- **Solução**: gere o servidor com o SAN correspondente e use a mesma URL:
  ```bash
  server serve --addr 0.0.0.0:8443 --san seu-host --san 127.0.0.1
  # cliente deve usar https://seu-host:8443
  ```
  Lembre: o SAN precisa casar com o **nome/IP** usado na URL, não com o endereço de escuta.

## `403 Forbidden` ao buscar `/key` ou `/artifact`

**Sintoma**: cliente autentica no TLS, mas recebe 403.

- **Causa**: o CN do certificado do cliente ≠ o build id da rota. Ou seja, você está usando
  o `client.exe` de um build para pegar a chave de outro.
- **Solução**: use o `client.exe` gerado para aquele build específico
  (`data/clients/<id>/`), ou passe o `--build-id` correto (que deve casar com o certificado
  embutido).

## Handshake TLS recusado / conexão cai antes de qualquer resposta

**Sintoma**: `curl -k` retorna código `000`; cliente sem resposta HTTP.

- **Causa**: as rotas exigem **mTLS** — sem certificado de cliente válido o handshake é
  rejeitado. Isso é esperado.
- **Solução**: apresente o certificado de cliente. Com `curl`:
  ```bash
  curl --cacert data/staging/<id>/ca.pem \
       --cert data/staging/<id>/identity.pem \
       --key  data/staging/<id>/identity.pem \
       https://127.0.0.1:8443/builds/<id>/key
  ```

## Cliente aborta com erro de identidade embutida

**Sintoma**: `"failed to load embedded client identity"` ou
`"failed to load embedded ca certificate"`.

- **Causa**: o `client.exe` foi compilado **sem** as variáveis `OMC_*` (é o *template* de
  dev, com CA/identidade vazias) e está sendo executado como se fosse funcional.
- **Solução**: use o `client.exe` produzido pelo `new-build` (que embute os assets do
  build). O binário em `target/.../release/client.exe` serve só para checar compilação.

## `new-build` falha ao compilar o cliente

**Sintoma**: `"client build failed with status ..."` ou
`"expected client binary not found at ..."`.

- **Causa 1**: o alvo Windows não está instalado para o `cargo` que o servidor invoca.
  - **Solução**: garanta o `cargo` do rustup (com o alvo) no `PATH` ao rodar o servidor:
    ```bash
    export PATH="$HOME/.cargo/bin:$PATH"
    rustup target add x86_64-pc-windows-gnu
    ```
- **Causa 2**: linker do mingw ausente (`x86_64-w64-mingw32-gcc` não encontrado).
  - **Solução**: `sudo apt install mingw-w64` e confira o `.cargo/config.toml`.
- **Causa 3**: o servidor está rodando fora da raiz do workspace e não acha o `cargo`/o
  crate `client`.
  - **Solução**: rode o `new-build` a partir da raiz do repositório (ou garanta que o
    layout do workspace esteja intacto — o orquestrador sobe dois níveis a partir de
    `crates/server`).

## `error[linker] x86_64-w64-mingw32-gcc: No such file`

- **Causa**: mingw-w64 não instalado ao cross-compilar o cliente.
- **Solução**: `sudo apt install mingw-w64`.

## `aws-lc-sys` volta a compilar (queria só `ring`)

**Sintoma**: build puxa `aws-lc-sys` e exige `cmake`/`nasm`.

- **Causa**: alguma dependência reativou a feature `aws_lc_rs` do `rustls` (as features se
  unificam no workspace).
- **Solução**: mantenha `rustls`/`tokio-rustls` com `default-features = false` + `ring` e o
  `axum-server` com `tls-rustls-no-provider`. Verifique:
  ```bash
  cargo tree -e no-dev | grep -i aws-lc   # deve sair vazio
  ```

## `failed to select a version for the requirement futures-macro = "=0.3.34"`

- **Causa**: índice de crates sem o `futures-util 0.3.34` (que fixa `futures-macro 0.3.34`).
- **Solução**: já tratado — `crates/server/Cargo.toml` fixa `futures-util = "=0.3.31"`.
  Ajuste a versão conforme o que o seu índice oferecer.

## `Address already in use` no `serve`

- **Causa**: a porta (padrão `8443`) já está ocupada, geralmente por um servidor anterior.
- **Solução**: encerre o processo antigo ou use outra porta:
  ```bash
  ss -ltnp | grep 8443
  server serve --addr 0.0.0.0:9443 --san seu-host
  ```

## Erro de conexão recusada no cliente

- **Causa**: o servidor não está no ar, ou a URL/porta estão erradas.
- **Solução**: confirme que o `serve` está rodando e que `--server`/`--server-url` apontam
  para o host:porta corretos.

## `rustup: command not found`

- **Causa**: o Rust da distro não gerencia alvos adicionais e não há `rustup`.
- **Solução**: instale o `rustup` (só no `$HOME`, sem alterar o shell):
  ```bash
  curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain 1.95.0
  export PATH="$HOME/.cargo/bin:$PATH"
  ```
