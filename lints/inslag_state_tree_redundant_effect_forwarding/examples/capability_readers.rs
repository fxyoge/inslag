#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message, state_tree::StateTreeInput)]
struct Start;
#[derive(Message, state_tree::StateTreeInput)]
struct Pause;

fn configure(config: &mut World) {
    config
        .add_state_tree_effect::<Start>("arcade.start")
        .add_state_tree_effect::<Pause>("arcade.pause");
}

fn first_reader(mut messages: MessageReader<Start>) {
    for _ in messages.read() {}
}

fn second_reader(mut messages: MessageReader<Start>) {
    for _ in messages.read() {}
}

fn main() {}
