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
        .add_state_tree_effect::<Command>("level.start")
        .add_state_tree_effect::<Command>("level.pause");
}

impl state_tree::StateTreeInput for Command {
    fn from_state_tree_input(
        context: &state_tree::StateTreeContext<'_>,
        _: &mut state_tree::StateTreeArgs,
    ) -> Result<Self, state_tree::StateTreeEffectError> {
        match context.effect_name() {
            Some("level.start") => Ok(Command::Start),
            Some("level.pause") => Ok(Command::Pause),
            _ => unreachable!(),
        }
    }
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
