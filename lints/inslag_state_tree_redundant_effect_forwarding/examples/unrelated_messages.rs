#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
struct Registered {
    action: String,
}

#[derive(Message)]
struct Unrelated {
    action: String,
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect::<Registered>("game.start")
        .add_state_tree_effect::<Registered>("game.pause");
}

impl state_tree::StateTreeInput for Registered {
    fn from_state_tree_input(
        context: &state_tree::StateTreeContext<'_>,
        _: &mut state_tree::StateTreeArgs,
    ) -> Result<Self, state_tree::StateTreeEffectError> {
        match context.effect_name() {
            Some("game.start") => Ok(Registered {
                action: "start".into(),
            }),
            Some("game.pause") => Ok(Registered {
                action: "pause".into(),
            }),
            _ => unreachable!(),
        }
    }
}

fn consume(mut commands: MessageReader<Unrelated>) {
    for command in commands.read() {
        match command.action.as_str() {
            "start" => {}
            "pause" => {}
            _ => {}
        }
    }
}

fn main() {}
