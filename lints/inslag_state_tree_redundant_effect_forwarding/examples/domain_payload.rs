#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

enum Mode {
    Marathon,
    Sprint,
}

#[derive(Message)]
struct SetMode {
    mode: Mode,
}

fn configure(config: &mut World) {
    config.add_state_tree_effect::<SetMode>("game.mode");
}

impl state_tree::StateTreeInput for SetMode {
    fn from_state_tree_input(
        _: &state_tree::StateTreeContext<'_>,
        args: &mut state_tree::StateTreeArgs,
    ) -> Result<Self, state_tree::StateTreeEffectError> {
        Ok(SetMode {
            mode: if args.take::<bool>("sprint")? {
                Mode::Sprint
            } else {
                Mode::Marathon
            },
        })
    }
}

fn consume(mut commands: MessageReader<SetMode>) {
    for command in commands.read() {
        match command.mode {
            Mode::Marathon => {}
            Mode::Sprint => {}
        }
    }
}

fn main() {}
