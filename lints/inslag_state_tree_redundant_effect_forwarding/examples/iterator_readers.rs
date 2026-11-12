#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
enum Command {
    Start,
    Pause,
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
            Some("arcade.start") => Ok(Command::Start),
            Some("arcade.pause") => Ok(Command::Pause),
            _ => unreachable!(),
        }
    }
}

fn enumerated(mut commands: MessageReader<Command>) {
    let pending = commands.read();
    for (index, command) in pending.enumerate() {
        let _first = index == 0;
        match command {
            Command::Start => {}
            Command::Pause => {}
        }
    }
}

fn with_id(mut commands: MessageReader<Command>) {
    for (command, _) in commands.read_with_id() {
        match command {
            Command::Start => {}
            Command::Pause => {}
        }
    }
}

fn main() {}
