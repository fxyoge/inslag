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
                action: "confirm".into(),
            })
        })
        .add_state_tree_effect("game.back", |_, _| {
            let action = String::from("back");
            let message = Command { action };
            Ok(message)
        });
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        let action = command.action.as_str();
        match action {
            "confirm" => {}
            "back" => {}
            _ => {}
        }
    }
}

fn main() {}
