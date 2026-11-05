#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
struct Command(String);

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("arcade.start", |_, _| Ok(Command("start".to_string())))
        .add_state_tree_effect("arcade.pause", |_, _| Ok(Command("pause".into())));
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command.0.as_str() {
            "start" => {}
            "pause" => {}
            _ => {}
        }
    }
}

fn main() {}
