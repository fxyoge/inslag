#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
enum Command {
    Start,
    Pause,
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("arcade.start", |_, _| Ok(Command::Start))
        .add_state_tree_effect("arcade.pause", |_, _| Ok(Command::Pause));
}

fn enumerated(mut commands: MessageReader<Command>) {
    let pending = commands.read();
    for (index, command) in pending.enumerate() {
        let _first = index == 0;
        match command {
            Command::Start => {}
            Command::Pause => {}
        }
    }
}

fn with_id(mut commands: MessageReader<Command>) {
    for (command, _) in commands.read_with_id() {
        match command {
            Command::Start => {}
            Command::Pause => {}
        }
    }
}

fn main() {}
