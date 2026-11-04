#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Resource)]
struct Clock {
    now: u64,
    samples: Vec<i32>,
}
struct Reading {
    now: u64,
    samples: Vec<i32>,
}
impl Clock {
    fn reading(&self) -> Reading {
        Reading {
            now: self.now,
            samples: self.samples.clone(),
        }
    }
}
fn inspect(clock: Res<Clock>) -> u64 {
    clock.reading().now
}
fn main() {}
