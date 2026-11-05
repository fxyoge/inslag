#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
enum Command {
    Start,
    Pause,
}

fn alternative_a(config: &mut World) {
    config.add_state_tree_effect("arcade.action", |_, _| Ok(Command::Start));
}

fn alternative_b(config: &mut World) {
    config.add_state_tree_effect("arcade.action", |_, _| Ok(Command::Pause));
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command {
            Command::Start => {}
            Command::Pause => {}
        }
    }
}

fn main() {}
