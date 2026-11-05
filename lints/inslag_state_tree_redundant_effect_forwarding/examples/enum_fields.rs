#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

enum Operation {
    Start,
    Pause,
}

#[derive(Message)]
struct Command {
    operation: Operation,
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("arcade.start", |_, _| {
            Ok(Command {
                operation: Operation::Start,
            })
        })
        .add_state_tree_effect("arcade.pause", |_, _| {
            Ok(Command {
                operation: Operation::Pause,
            })
        });
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command.operation {
            Operation::Start => {}
            Operation::Pause => {}
        }
    }
}

fn main() {}
