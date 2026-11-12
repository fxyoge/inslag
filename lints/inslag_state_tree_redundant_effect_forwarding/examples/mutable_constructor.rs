#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
struct Command {
    action: String,
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect::<Command>("game.start")
        .add_state_tree_effect::<Command>("game.pause");
}

impl state_tree::StateTreeInput for Command {
    fn from_state_tree_input(
        context: &state_tree::StateTreeContext<'_>,
        args: &mut state_tree::StateTreeArgs,
    ) -> Result<Self, state_tree::StateTreeEffectError> {
        match context.effect_name() {
            Some("game.start") => {
                let mut message = Command {
                    action: "start".into(),
                };
                message.action = args.take("action")?;
                Ok(message)
            }
            Some("game.pause") => Ok(Command {
                action: "pause".into(),
            }),
            _ => unreachable!(),
        }
    }
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command.action.as_str() {
            "start" => {}
            "pause" => {}
            _ => {}
        }
    }
}

fn main() {}
