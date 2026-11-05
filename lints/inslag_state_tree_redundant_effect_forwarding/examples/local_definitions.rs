#![allow(dead_code)]

use bevy_ecs::prelude::*;

#[derive(Message)]
pub enum Command {
    Start,
    Pause,
}

pub trait StateTreeExt {
    fn add_state_tree_effect(
        &mut self,
        name: &str,
        constructor: impl Fn() -> Result<Command, ()>,
    ) -> &mut Self;
}

impl StateTreeExt for World {
    fn add_state_tree_effect(&mut self, _: &str, _: impl Fn() -> Result<Command, ()>) -> &mut Self {
        self
    }
}

fn configure(world: &mut World) {
    world
        .add_state_tree_effect("arcade.start", || Ok(Command::Start))
        .add_state_tree_effect("arcade.pause", || Ok(Command::Pause));
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
