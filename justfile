set unstable
set lists
set lazy

default:
  @just --list

bench: \
  _bench-loot-execution \
  _bench-luau-expressions

docs: \
  _watch-docs

[script]
[working-directory: 'crates/inslag_format_loot']
fuzz-loot-format:
  cargo +nightly-2026-08-20 fuzz run parse

lint: \
  _lint-workspace \
  _lint-lints \
  _lint-dylint

test: \
  _test-workspace \
  _test-lints

_lint-dylint:
    cargo dylint --all --fail-on-no-libraries -- --workspace --all-targets --locked --config 'build.rustflags=["-Dwarnings","-Zalways-encode-mir"]'

[working-directory: 'lints']
_lint-lints:
    cargo fmt --all --check
    cargo clippy --locked --lib --tests -- -D warnings

_lint-workspace:
    cargo fmt --all --check
    cargo clippy --locked --all-targets -- -D warnings

_bench-loot-execution:
  cargo bench -p inslag_loot --bench execution

_bench-luau-expressions:
  cargo bench -p inslag_luau --bench expressions

[script]
[cache(inputs = [split(`rg --files --glob '**/*.rs'`)], outputs = "target-docs")]
_build-docs:
    CARGO_TARGET_DIR=target-docs cargo doc --workspace --no-deps --locked

[working-directory: 'lints']
_test-lints:
  cargo test --locked --lib

_test-workspace:
  cargo test --workspace

[script]
_watch-docs:
    setsid sh -c 'while sleep 5; do just _build-docs; done' &
    watcher_pid=$!
    cleanup() {
        /bin/kill -- -"$watcher_pid" 2>/dev/null || true
    }
    trap cleanup EXIT
    trap 'exit 130' INT TERM
    python3 -m http.server 8000 --directory target-docs/doc
