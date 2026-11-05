# bevy_indirect_system_registration

### What it does

Checks for helper calls that return system configurations inside Bevy's `Plugin::build`. This includes:

- Functions and methods returning `ScheduleConfigs<ScheduleSystem>`, including type aliases.
- Helpers returning `impl IntoScheduleConfigs<ScheduleSystem, M>`, including generic helpers.
- Calls through function pointers.
- Helper calls nested in system tuples or chained configurations.
- Helper results assigned to local variables before registration.

Bevy's configuration methods (`chain`, `in_set`, `run_if`, etc.), system-set configurations, and helpers that mutate the `App` without returning system configurations are not flagged.

### Why is this bad?

Helper functions obfuscate the structure of a Bevy plugin.

### Example

```rust,no_run
fn a() {}
fn b() {}
fn systems() -> ScheduleConfigs<ScheduleSystem> {
    (a, b).chain()
}

struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, systems());
    }
}
```

Use instead:

```rust,no_run
fn a() {}
fn b() {}

struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (a, b).chain());
    }
}
```
