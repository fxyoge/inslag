#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
struct Command {
    action: String,
}

fn make_message(_: String) -> Command {
    Command {
        action: String::new(),
    }
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("game.start", |_, _| Ok(make_message("start".into())))
        .add_state_tree_effect("game.pause", |_, _| Ok(make_message("pause".into())));
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
