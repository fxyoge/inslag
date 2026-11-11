# bevy_unnecessary_mut_query

### What it does

Checks for mutable query fields used only through shared access. Skips escapes and uses it cannot account for.

### Why is this bad?

Unnecessary write access prevents otherwise compatible systems from running together.

### Example

```rust,no_run
fn inspect(mut query: Query<&mut Health>) {
    for health in &mut query {
        show(health.current);
    }
}
```

Use `Query<&Health>` instead. Other fields in the query can remain mutable.
