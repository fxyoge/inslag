#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
enum Command {
    Start,
    Pause,
}

macro_rules! possibly_return {
    ($pause:expr) => {
        if $pause {
            return Ok(Command::Pause);
        }
    };
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect::<Command>("arcade.start")
        .add_state_tree_effect::<Command>("arcade.pause");
}

impl state_tree::StateTreeInput for Command {
    fn from_state_tree_input(
        context: &state_tree::StateTreeContext<'_>,
        args: &mut state_tree::StateTreeArgs,
    ) -> Result<Self, state_tree::StateTreeEffectError> {
        match context.effect_name() {
            Some("arcade.start") => {
                possibly_return!(args.take::<bool>("pause")?);
                Ok(Command::Start)
            }
            Some("arcade.pause") => Ok(Command::Pause),
            _ => unreachable!(),
        }
    }
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command {
            Command::Start => {}
            Command::Pause => {}
        }
    }
}

fn main() {}
