#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

struct Source;

impl Source {
    fn to_owned(&self) -> String {
        String::new()
    }
}

#[derive(Message)]
struct Command {
    action: String,
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("game.start", |_, _| {
            Ok(Command {
                action: Source.to_owned(),
            })
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
