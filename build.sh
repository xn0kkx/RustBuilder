#!/usr/bin/env bash

set -euo pipefail

PROJECT_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_ROOT"

PROFILE="--release"
CLIENT_TARGET="--target x86_64-pc-windows-gnu"
CLIENT_FEATURES=()
CLIENT_DEFAULT_FEATURES=("--features" "antivm")

for arg in "$@"; do
    case "$arg" in
        --debug)
            PROFILE=""
            CLIENT_TARGET=""
            ;;
        --obfs)
            CLIENT_FEATURES+=("obfs")
            ;;
        --no-antivm)
            CLIENT_DEFAULT_FEATURES=()
            ;;
        *)
            printf 'Uso: %s [--debug] [--obfs] [--no-antivm]\n' "$0" >&2
            exit 2
            ;;
    esac
done

printf 'Compilando server...\n'
cargo build $PROFILE -p server

printf 'Compilando client...\n'
cargo build $PROFILE -p client $CLIENT_TARGET "${CLIENT_DEFAULT_FEATURES[@]}" \
    $(if [[ ${#CLIENT_FEATURES[@]} -gt 0 ]]; then printf '%s' '--features'; fi) \
    "${CLIENT_FEATURES[@]}"

printf 'Build concluido.\n'