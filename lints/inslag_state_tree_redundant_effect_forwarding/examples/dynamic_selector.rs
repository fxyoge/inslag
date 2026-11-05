#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
struct Command {
    action: String,
}

fn configure(config: &mut World) {
    config.add_state_tree_effect("game.action", |_, args| {
        Ok(Command {
            action: args.take("action")?,
        })
    });
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command.action.as_str() {
            "quit" => {}
            "fullscreen" => {}
            _ => {}
        }
    }
}

fn main() {}
