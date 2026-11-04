#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Component)]
struct Position(i32);
fn inspect(query: Query<(Entity, &Position)>) -> Vec<Entity> {
    query
        .iter()
        .filter(|(_, position)| position.0 > 0)
        .map(|(entity, _)| entity)
        .collect()
}
fn main() {}
