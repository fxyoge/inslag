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
                action: "confirm".into(),
            }),
            Some("game.back") => {
                let action = String::from("back");
                let message = Command { action };
                Ok(message)
            }
            _ => unreachable!(),
        }
    }
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        let action = command.action.as_str();
        match action {
            "confirm" => {}
            "back" => {}
            _ => {}
        }
    }
}

fn main() {}
