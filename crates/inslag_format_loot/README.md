# inslag_format_loot

Parse `.loot` files without Bevy or a loot runtime. Owned syntax, source spans, and structured errors.

```rust
let document = inslag_format_loot::parse("table \"rewards\"\n  gold\n").unwrap();
assert_eq!(document.tables[0].name.value, "rewards");
```
