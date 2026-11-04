# unchained_builder

### What it does

Checks for consecutive builder calls that should be chained. Supports:

- Non-consuming builders (`fn option(&mut self) -> &mut Self`)
- Consuming builders (`fn option(self) -> Self`)

### Why is this bad?

Splitting a builder chain obscures a single configuration.

### Example

```rust,no_run
fn configure(builder: &mut Builder) {
    builder.first();
    builder.second();
}
```

Use instead:

```rust,no_run
fn configure(builder: &mut Builder) {
    builder.first().second();
}
```
