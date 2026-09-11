#!/usr/bin/env bash

set -euo pipefail

PROJECT_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_ROOT"

PROFILE="--release"
CLIENT_TARGET="--target x86_64-pc-windows-gnu"

if [[ "${1:-}" == "--debug" ]]; then
    PROFILE=""
    CLIENT_TARGET=""
elif [[ $# -gt 0 ]]; then
    printf 'Uso: %s [--debug]\n' "$0" >&2
    exit 2
fi

printf 'Compilando server...\n'
cargo build $PROFILE -p server

printf 'Compilando client...\n'
cargo build $PROFILE -p client $CLIENT_TARGET

printf 'Build concluido.\n'