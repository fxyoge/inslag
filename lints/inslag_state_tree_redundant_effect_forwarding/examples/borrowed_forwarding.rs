#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
struct Command {
    action: &'static str,
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("game.confirm", |_, _| Ok(Command { action: "confirm" }))
        .add_state_tree_effect("game.back", |_, _| Ok(Command { action: "back" }));
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command.action {
            "confirm" | "back" => {}
            _ => {}
        }
    }
}

fn main() {}
