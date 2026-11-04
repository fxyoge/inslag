#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Resource)]
struct Position {
    x: i32,
    y: i32,
}
struct Coordinates {
    x: i32,
    y: i32,
}
impl Position {
    fn coordinates(&self) -> Coordinates {
        Coordinates {
            x: self.x,
            y: self.y,
        }
    }
}
fn inspect(position: Res<Position>) -> i32 {
    position.coordinates().x
}
fn main() {}
