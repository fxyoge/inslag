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
        .add_state_tree_effect::<Command<u32>>("arcade.start")
        .add_state_tree_effect::<Command<u64>>("arcade.pause");
}

impl<T: Send + Sync + 'static> state_tree::StateTreeInput for Command<T> {
    fn from_state_tree_input(
        context: &state_tree::StateTreeContext<'_>,
        _: &mut state_tree::StateTreeArgs,
    ) -> Result<Self, state_tree::StateTreeEffectError> {
        match context.effect_name() {
            Some("arcade.start") => Ok(Command::<T>::Start(PhantomData)),
            Some("arcade.pause") => Ok(Command::<T>::Pause(PhantomData)),
            _ => unreachable!(),
        }
    }
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
