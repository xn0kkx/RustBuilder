# Proteção anti-VM (feature `antivm`)

O cliente pode incluir a biblioteca [`antivm`](https://github.com/northernboykisser/anti-vm-rust)
para encerrar a execução em ambientes indesejados (VirtualBox/VMware, etc.). A proteção é
opcional, controlada por uma **feature de compilação**, para que builds de desenvolvimento
compilem e rodem sem ela.

## Feature flag

Definida em `crates/client/Cargo.toml`:

```toml
[features]
default = ["antivm"]
antivm = ["dep:antivm"]

[target.'cfg(windows)'.dependencies]
antivm = { version = "1.2", optional = true }
```

- **Padrão (produção)**: a feature `antivm` está **ligada**. A proteção é chamada no início do
  `main` do cliente, mas só tem efeito no **alvo Windows** (o gate é
  `#[cfg(all(windows, feature = "antivm"))]`).
- **Desligada (desenvolvimento/teste)**: compile com `--no-default-features` para excluir o
  crate `antivm` e a chamada de proteção.

```bash
# produção (antivm on) — via cargo direto
cargo build -p client --release --target x86_64-pc-windows-gnu

# desenvolvimento (antivm off)
cargo build -p client --no-default-features
cargo build -p client --no-default-features --target x86_64-pc-windows-gnu
```

O orquestrador (`server new-build`) compila o cliente com as features padrão (antivm on). Para
gerar um cliente de teste sem antivm pelo orquestrador, use a flag `--no-antivm`:

```bash
./target/release/server new-build --artifact payload.txt --uid dev --no-antivm \
  --target x86_64-pc-windows-gnu
```

## Configuração dos filtros

A chamada em `crates/client/src/main.rs` habilita apenas o filtro de VM e desliga os demais
(IP/geo, HTTP, rede, resolução, CPU, RAM), porque os defaults do crate encerram o processo em
máquinas/redes comuns (por ex.: IP de país da UE/OTAN, ou menos de 4 GB de RAM). Ajuste os
`set_*` conforme a necessidade:

```rust
antivm::ProtectionBuilder::new()
    .set_vm(true)
    .set_ip(false)
    .set_http(false)
    .set_network(false)
    .set_screen(false)
    .set_cpu(false)
    .set_ram(false)
    .init();
```

## Cópia local corrigida (vendor)

O `antivm` 1.2.0 não cross-compila de um host Linux: o `build.rs` original do crate faz
`use winapi::um::winuser::*;` no topo do arquivo, e build scripts são compilados para o **host**
— o módulo `winapi::um` só existe em host Windows.

Por isso o crate é usado a partir de uma cópia local em `vendor/antivm/`, redirecionada por
`[patch.crates-io]` no `Cargo.toml` raiz. A única mudança em relação ao upstream é envolver o
uso da WinAPI no `build.rs` num bloco `#[cfg(windows)]`, permitindo que o build script compile
em host Linux (cross-compilando o alvo Windows) sem alterar o comportamento em host Windows.
