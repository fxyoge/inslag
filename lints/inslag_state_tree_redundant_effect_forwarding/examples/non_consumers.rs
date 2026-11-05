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

fn inspect(command: &Command) -> &'static str {
    match command {
        Command::Start => "start",
        Command::Pause => "pause",
    }
}

fn fabricated_inside_reader(mut commands: MessageReader<Command>) {
    for _ in commands.read() {
        let command = Command::Start;
        match command {
            Command::Start => {}
            Command::Pause => {}
        }
    }
}

fn main() {}
