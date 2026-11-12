#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

enum Operation {
    Start,
    Pause,
}

#[derive(Message)]
struct Command {
    operation: Operation,
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect::<Command>("arcade.start")
        .add_state_tree_effect::<Command>("arcade.pause");
}

impl state_tree::StateTreeInput for Command {
    fn from_state_tree_input(
        context: &state_tree::StateTreeContext<'_>,
        _: &mut state_tree::StateTreeArgs,
    ) -> Result<Self, state_tree::StateTreeEffectError> {
        match context.effect_name() {
            Some("arcade.start") => Ok(Command {
                operation: Operation::Start,
            }),
            Some("arcade.pause") => Ok(Command {
                operation: Operation::Pause,
            }),
            _ => unreachable!(),
        }
    }
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command.operation {
            Operation::Start => {}
            Operation::Pause => {}
        }
    }
}

fn main() {}
