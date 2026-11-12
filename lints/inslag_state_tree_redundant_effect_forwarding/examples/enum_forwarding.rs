#![allow(dead_code)]

use bevy_app::App;
use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message, state_tree::StateTreeInput)]
enum Command {
    #[state_tree(name = "arcade.start")]
    Start {},
    #[state_tree(name = "arcade.pause")]
    Pause {},
}

fn consume(mut commands: MessageReader<Command>) {
    for command in commands.read() {
        match command {
            Command::Start {} => {}
            Command::Pause {} => {}
        }
    }
}

fn configure(config: &mut App) {
    config
        .add_state_tree_effect::<Command>("arcade.start")
        .add_state_tree_effect::<Command>("arcade.pause");
}

fn main() {}
