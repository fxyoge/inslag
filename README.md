# inslag

Opinionated asset-backed game authoring for Bevy.

- [inslag_luau](crates/inslag_luau/): portable, compile-once Luau expressions with native fast paths.
- [inslag_loot](crates/inslag_loot/): weighted choices, reusable tables, and relationships between generated results.
- [inslag_persist](crates/inslag_persist/): SQLite-backed saves for Bevy.

Disclaimer: this is not production ready. Expect breaking changes.

## Lints

Dylint is supported!

```toml
[workspace.metadata.dylint]
libraries = [
    { git = "https://github.com/fxyoge/inslag.git", rev = "<sha>", pattern = "lints/*" },
]
```

Then run:

```sh
cargo dylint --all -- --all-targets --locked --config 'build.rustflags=["-Zalways-encode-mir"]'
```

`-Zalways-encode-mir` enables checks that inspect dependency method bodies.
