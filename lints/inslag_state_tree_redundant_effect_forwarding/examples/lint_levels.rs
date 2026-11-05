#![allow(dead_code)]
#![deny(unfulfilled_lint_expectations)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
enum Command {
    Start,
    Pause,
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("level.start", |_, _| Ok(Command::Start))
        .add_state_tree_effect("level.pause", |_, _| Ok(Command::Pause));
}

#[cfg_attr(
    dylint_lib = "inslag_state_tree_redundant_effect_forwarding",
    allow(
        inslag_state_tree_redundant_effect_forwarding,
        reason = "exercise node-scoped lint levels"
    )
)]
fn allowed(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command {
            Command::Start => {}
            Command::Pause => {}
        }
    }
}

#[cfg_attr(
    dylint_lib = "inslag_state_tree_redundant_effect_forwarding",
    expect(
        inslag_state_tree_redundant_effect_forwarding,
        reason = "exercise node-scoped expectations"
    )
)]
fn expected(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command {
            Command::Start => {}
            Command::Pause => {}
        }
    }
}

fn forbidden(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command {
            Command::Start => {}
            Command::Pause => {}
        }
    }
}

fn main() {}
