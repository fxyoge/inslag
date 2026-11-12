#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
enum Command {
    Start,
    Pause,
}

fn alternative_a(config: &mut World) {
    config.add_state_tree_effect::<Command>("arcade.action");
}

fn alternative_b(config: &mut World) {
    config.add_state_tree_effect::<Command>("arcade.action");
}

impl state_tree::StateTreeInput for Command {
    fn from_state_tree_input(
        context: &state_tree::StateTreeContext<'_>,
        args: &mut state_tree::StateTreeArgs,
    ) -> Result<Self, state_tree::StateTreeEffectError> {
        match context.effect_name() {
            Some("arcade.action") => Ok(if args.take("pause")? {
                Command::Pause
            } else {
                Command::Start
            }),
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
