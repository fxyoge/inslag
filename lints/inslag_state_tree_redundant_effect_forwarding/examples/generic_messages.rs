#![allow(dead_code)]

use std::marker::PhantomData;

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
enum Command<T: Send + Sync + 'static> {
    Start(PhantomData<T>),
    Pause(PhantomData<T>),
}

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("arcade.start", |_, _| {
            Ok(Command::<u32>::Start(PhantomData))
        })
        .add_state_tree_effect("arcade.pause", |_, _| {
            Ok(Command::<u64>::Pause(PhantomData))
        });
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
