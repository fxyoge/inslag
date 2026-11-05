#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
struct Command {
    action: String,
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("game.confirm", |_, _| {
            Ok(Command {
                action: "confirm".to_owned(),
            })
        })
        .add_state_tree_effect("game.back", |_, _| {
            Ok(Command {
                action: "back".into(),
            })
        });
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        if command.action == "confirm" {
            std::hint::black_box(command);
        }
        if "back" != command.action {
            std::hint::black_box(command);
        }
        if let "confirm" = command.action.as_str() {
            std::hint::black_box(command);
        }
    }
}

fn main() {}
