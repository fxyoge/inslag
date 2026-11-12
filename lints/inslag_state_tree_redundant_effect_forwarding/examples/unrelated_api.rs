#![allow(dead_code)]

use bevy_ecs::prelude::*;

#[derive(Message)]
enum Command {
    Start,
    Pause,
}

struct FakeConfig;

impl FakeConfig {
    fn add_state_tree_effect<M: Message>(&mut self, _: &str) -> &mut Self {
        self
    }
}

mod lookalike {
    pub trait StateTreeExt {
        fn add_state_tree_effect<M: bevy_ecs::message::Message>(&mut self, name: &str)
        -> &mut Self;
    }

    impl StateTreeExt for super::FakeConfig {
        fn add_state_tree_effect<M: bevy_ecs::message::Message>(&mut self, _: &str) -> &mut Self {
            self
        }
    }
}

fn configure(config: &mut FakeConfig) {
    config
        .add_state_tree_effect::<Command>("arcade.start")
        .add_state_tree_effect::<Command>("arcade.pause");
    lookalike::StateTreeExt::add_state_tree_effect::<Command>(config, "arcade.start");
    lookalike::StateTreeExt::add_state_tree_effect::<Command>(config, "arcade.pause");
}

impl state_tree::StateTreeInput for Command {
    fn from_state_tree_input(
        context: &state_tree::StateTreeContext<'_>,
        _: &mut state_tree::StateTreeArgs,
    ) -> Result<Self, state_tree::StateTreeEffectError> {
        match context.effect_name() {
            Some("arcade.start") => Ok(Command::Start),
            Some("arcade.pause") => Ok(Command::Pause),
            _ => unreachable!(),
        }
    }
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
