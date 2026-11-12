#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
struct Command {
    action: String,
    value: String,
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect::<Command>("text.a")
        .add_state_tree_effect::<Command>("text.b");
}

impl state_tree::StateTreeInput for Command {
    fn from_state_tree_input(
        context: &state_tree::StateTreeContext<'_>,
        args: &mut state_tree::StateTreeArgs,
    ) -> Result<Self, state_tree::StateTreeEffectError> {
        match context.effect_name() {
            Some("text.a") => Ok(Command {
                action: "a".into(),
                value: args.take("value")?,
            }),
            Some("text.b") => Ok(Command {
                action: "b".into(),
                value: args.take("value")?,
            }),
            _ => unreachable!(),
        }
    }
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command.value.as_str() {
            "a" => {}
            "b" => {}
            _ => {}
        }
        let _ = command.action.len();
    }
}

fn main() {}
