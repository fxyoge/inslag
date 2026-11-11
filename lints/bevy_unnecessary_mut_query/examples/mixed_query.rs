#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Component)]
struct Progress(u32);
#[derive(Component)]
struct Sprite(u32);

fn update(mut query: Query<(&mut Progress, &mut Sprite)>) {
    for (mut progress, sprite) in &mut query {
        progress.0 += sprite.0;
    }
}

fn main() {}
