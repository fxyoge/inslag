#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message, PartialEq)]
enum Command {
    Start,
    Retry { seed: u32 },
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("arcade.start", |_, _| Ok(Command::Start))
        .add_state_tree_effect("arcade.retry", |_, args| {
            Ok(Command::Retry {
                seed: args.take("seed")?,
            })
        });
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
