#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
output=src/backend/sqlite/queries.rs
temporary=$(mktemp -d)
trap 'rm -rf -- "$temporary"' EXIT

cp -R sqlite "$temporary/sqlite"
sqlc -f "$temporary/sqlite/sqlc.yaml" generate
rustfmt --edition 2024 "$temporary/$output"

if [[ ${1:-} == --check ]]; then
    diff -u "$output" "$temporary/$output"
else
    mkdir -p "$(dirname "$output")"
    cp "$temporary/$output" "$output"
fi
