#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"
temporary=$(mktemp -d)
trap 'rm -rf -- "$temporary"' EXIT

cp -R migrations queries sqlc.yaml "$temporary/"
sqlc -f "$temporary/sqlc.yaml" generate
rustfmt --edition 2024 "$temporary/queries.rs"

if [[ ${1:-} == --check ]]; then
    diff -u queries.rs "$temporary/queries.rs"
else
    cp "$temporary/queries.rs" queries.rs
fi
