#![allow(dead_code)]

use bevy_ecs::prelude::*;
use state_tree::StateTreeExt;

#[derive(Message)]
struct Start;
#[derive(Message)]
struct Pause;

fn configure(config: &mut World) {
    config
        .add_state_tree_effect("arcade.start", |_, _| Ok(Start))
        .add_state_tree_effect("arcade.pause", |_, _| Ok(Pause));
}

fn first_reader(mut messages: MessageReader<Start>) {
    for _ in messages.read() {}
}

fn second_reader(mut messages: MessageReader<Start>) {
    for _ in messages.read() {}
}

fn main() {}
