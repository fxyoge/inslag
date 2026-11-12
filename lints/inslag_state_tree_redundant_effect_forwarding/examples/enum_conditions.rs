#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message, PartialEq, state_tree::StateTreeInput)]
enum Command {
    #[state_tree(name = "arcade.start")]
    Start,
    #[state_tree(name = "arcade.retry")]
    Retry { seed: u32 },
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect::<Command>("arcade.start")
        .add_state_tree_effect::<Command>("arcade.retry");
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        if let Command::Retry { seed } = command {
            let _ = seed;
        }
        if *command == Command::Start {
            std::hint::black_box(command);
        }
        if Command::Start != *command {
            std::hint::black_box(command);
        }
    }
}

fn main() {}
