# bevy_component_without_producer

### What it does

Checks for private Bevy components used in queries without a known producer. Skips construction, registration, escapes, and uses it cannot account for, including unfamiliar macro integrations.

### Why is this bad?

A component with no producer implies missing initialization or obsolete ECS code.

### Example

```rust,no_run
#[derive(Component)]
struct Ready;

fn update(ready: Query<Entity, With<Ready>>) {
    for entity in &ready {
        update_ready(entity);
    }
}
```

If `Ready` is still needed, insert it when an entity becomes ready. Otherwise, remove the obsolete consumer or constraint.
