#![allow(dead_code)]

use std::marker::PhantomData;

use bevy_ecs::prelude::*;
use state_tree::{StateTreeExt, StateTreeInput};

#[derive(Message, StateTreeInput)]
enum Command<T: Send + Sync + 'static> {
    #[state_tree(name = "arcade.start")]
    Start(#[state_tree(skip)] PhantomData<T>),
    #[state_tree(name = "arcade.pause")]
    Pause(#[state_tree(skip)] PhantomData<T>),
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect::<Command<u32>>("arcade.start")
        .add_state_tree_effect::<Command<u32>>("arcade.pause");
}

fn consume(mut commands: MessageReader<Command<u32>>) {
    for command in commands.read() {
        match command {
            Command::Start(_) => {}
            Command::Pause(_) => {}
        }
    }
}

fn main() {}
