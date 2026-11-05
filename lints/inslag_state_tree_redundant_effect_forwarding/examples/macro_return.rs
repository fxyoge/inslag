#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
enum Command {
    Start,
    Pause,
}

macro_rules! possibly_return {
    ($pause:expr) => {
        if $pause {
            return Ok(Command::Pause);
        }
    };
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("arcade.start", |_, args| {
            possibly_return!(args.take::<bool>("pause")?);
            Ok(Command::Start)
        })
        .add_state_tree_effect("arcade.pause", |_, _| Ok(Command::Pause));
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
