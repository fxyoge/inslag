#![allow(dead_code)]

use bevy_ecs::prelude::*;

#[derive(Message)]
pub enum Command {
    Start,
    Pause,
}

pub trait StateTreeExt {
    fn add_state_tree_effect<M: Message + StateTreeInput>(&mut self, name: &str) -> &mut Self;
}

impl StateTreeExt for World {
    fn add_state_tree_effect<M: Message + StateTreeInput>(&mut self, _: &str) -> &mut Self {
        self
    }
}

pub struct StateTreeContext;

impl StateTreeContext {
    pub fn effect_name(&self) -> Option<&str> {
        None
    }
}

pub trait StateTreeInput: Sized {
    fn from_state_tree_input(context: &StateTreeContext) -> Result<Self, ()>;
}

impl StateTreeInput for Command {
    fn from_state_tree_input(context: &StateTreeContext) -> Result<Self, ()> {
        match context.effect_name() {
            Some("arcade.start") => Ok(Command::Start),
            Some("arcade.pause") => Ok(Command::Pause),
            _ => unreachable!(),
        }
    }
}

fn configure(world: &mut World) {
    world
        .add_state_tree_effect::<Command>("arcade.start")
        .add_state_tree_effect::<Command>("arcade.pause");
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
