#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
struct Command {
    action: String,
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect::<Command>("game.confirm")
        .add_state_tree_effect::<Command>("game.back");
}

impl state_tree::StateTreeInput for Command {
    fn from_state_tree_input(
        context: &state_tree::StateTreeContext<'_>,
        _: &mut state_tree::StateTreeArgs,
    ) -> Result<Self, state_tree::StateTreeEffectError> {
        match context.effect_name() {
            Some("game.confirm") => Ok(Command {
                action: "confirm".to_owned(),
            }),
            Some("game.back") => Ok(Command {
                action: "back".into(),
            }),
            _ => unreachable!(),
        }
    }
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        if command.action == "confirm" {
            std::hint::black_box(command);
        }
        if "back" != command.action {
            std::hint::black_box(command);
        }
        if let "confirm" = command.action.as_str() {
            std::hint::black_box(command);
        }
    }
}

fn main() {}
