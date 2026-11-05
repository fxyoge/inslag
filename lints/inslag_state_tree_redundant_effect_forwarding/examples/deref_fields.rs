#![allow(dead_code)]

use bevy_ecs::prelude::*;

#[derive(Resource)]
struct Facts {
    score: u32,
    lines: u32,
    level: u32,
    phase: &'static str,
}

fn inspect(facts: Res<Facts>) {
    match facts.phase {
        "playing" => {}
        "paused" => {}
        _ => {}
    }
}

fn main() {}
