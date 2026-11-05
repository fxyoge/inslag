#![allow(dead_code)]

use bevy_ecs::prelude::*;

#[derive(Message)]
enum Command {
    Start,
    Pause,
}

struct FakeConfig;

impl FakeConfig {
    fn add_state_tree_effect(&mut self, _: &str, _: impl Fn() -> Command) -> &mut Self {
        self
    }
}

mod lookalike {
    pub trait StateTreeExt {
        fn add_state_tree_effect(
            &mut self,
            name: &str,
            constructor: impl Fn() -> super::Command,
        ) -> &mut Self;
    }

    impl StateTreeExt for super::FakeConfig {
        fn add_state_tree_effect(&mut self, _: &str, _: impl Fn() -> super::Command) -> &mut Self {
            self
        }
    }
}

fn configure(config: &mut FakeConfig) {
    config
        .add_state_tree_effect("arcade.start", || Command::Start)
        .add_state_tree_effect("arcade.pause", || Command::Pause);
    lookalike::StateTreeExt::add_state_tree_effect(config, "arcade.start", || Command::Start);
    lookalike::StateTreeExt::add_state_tree_effect(config, "arcade.pause", || Command::Pause);
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
