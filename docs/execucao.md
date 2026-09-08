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
| `--out <arquivo>` | (obrigatório no download) | onde gravar o arquivo descriptografado |
| `--upload <arquivo>` | (opcional) | envia este arquivo como diagnóstico cifrado (modo upload) |
| `--server <url>` | valor embutido no build | sobrescreve a URL do servidor |
| `--build-id <id>` | valor embutido no build | sobrescreve o id do build |

O cliente conecta por mTLS (com a identidade embutida), baixa o artefato cifrado e a chave
privada, **descriptografa localmente** e grava em `--out`.

### Upload de diagnósticos

Com `--upload <arquivo>`, o cliente cifra o arquivo com a chave pública PGP do build (embutida
no binário) e o envia ao servidor em chunks (`POST /builds/:id/diagnostics`); o servidor grava
o blob cifrado em `data/diagnostics/<id>/`. O texto puro nunca sai da máquina do cliente. Esse
modo é um wrapper fino sobre a função reutilizável `upload_diagnostic`, que a main principal
poderá chamar diretamente.

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
