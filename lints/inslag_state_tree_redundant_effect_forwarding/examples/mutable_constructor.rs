#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
struct Command {
    action: String,
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("game.start", |_, args| {
            let mut message = Command {
                action: "start".into(),
            };
            message.action = args.take("action")?;
            Ok(message)
        })
        .add_state_tree_effect("game.pause", |_, _| {
            Ok(Command {
                action: "pause".into(),
            })
        });
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command.action.as_str() {
            "start" => {}
            "pause" => {}
            _ => {}
        }
    }
}

fn main() {}
