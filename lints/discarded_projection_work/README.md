# discarded_projection_work

### What it does

Checks for calls that build strings or collections in fields the caller never uses. Skips cheap scalar results, known-empty inputs, shared prerequisites, and cases where effects or result uses are unclear.

### Why is this bad?

Building fields just to throw them away hides what a caller needs and can waste work. Compute only the needed values, or save those values before mutation.

### Example

```rust,no_run
fn clock(game: &Game) -> u64 {
    game.snapshot().tick
}
```

Use instead:

```rust,no_run
fn clock(game: &Game) -> u64 {
    game.tick()
}
```
