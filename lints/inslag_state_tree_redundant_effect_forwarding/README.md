# inslag_state_tree_redundant_effect_forwarding

### What it does

Checks for message consumers that forward operations already selected by state-tree effect registration. This includes:

- Enum variants fixed by different authored effects.
- Constant string or enum fields in shared message types.
- Dispatch through `match`, `if let`, and equality comparisons.

Registrations and consumers must be in the same crate, and consumed values must be traceable to a `MessageReader`. Aliases emitting the same discriminator, runtime payloads, and unrelated message types are not flagged.

### Why is this bad?

`inslag_state_tree` already handles effect routing; forwarding effects to another router unnecessarily duplicates work and obfuscates the source of the effect.

### Example

```rust,no_run
#[derive(Message)]
enum Command {
    Start,
    Pause,
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("arcade.start", |_, _| Ok(Command::Start))
        .add_state_tree_effect("arcade.pause", |_, _| Ok(Command::Pause));
}

fn handle_commands(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command {
            Command::Start => start_game(),
            Command::Pause => pause_game(),
        }
    }
}
```

Use instead:

```rust,no_run
#[derive(Message)]
struct Start;
#[derive(Message)]
struct Pause;

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("arcade.start", |_, _| Ok(Start))
        .add_state_tree_effect("arcade.pause", |_, _| Ok(Pause));
}

fn start(mut messages: MessageReader<Start>) {
    for _ in messages.read() {
        start_game();
    }
}

fn pause(mut messages: MessageReader<Pause>) {
    for _ in messages.read() {
        pause_game();
    }
}
```
