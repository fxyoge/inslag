#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::{StateTreeExt, StateTreeInput};

#[derive(Message, StateTreeInput)]
enum Command {
    #[state_tree(name = "arcade.start")]
    Start {},
    #[state_tree(name = "arcade.pause")]
    Pause {},
}

fn configure(config: &mut World) {
    config.add_state_tree_effect::<Command>("arcade.start");
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command {
            Command::Start {} => {}
            Command::Pause {} => {}
        }
    }
}

fn main() {}
