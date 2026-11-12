#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
struct Command {
    action: String,
}

fn make_message(_: String) -> Command {
    Command {
        action: String::new(),
    }
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect::<Command>("game.start")
        .add_state_tree_effect::<Command>("game.pause");
}

impl state_tree::StateTreeInput for Command {
    fn from_state_tree_input(
        context: &state_tree::StateTreeContext<'_>,
        _: &mut state_tree::StateTreeArgs,
    ) -> Result<Self, state_tree::StateTreeEffectError> {
        match context.effect_name() {
            Some("game.start") => Ok(make_message("start".into())),
            Some("game.pause") => Ok(make_message("pause".into())),
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
