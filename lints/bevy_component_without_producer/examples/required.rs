#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Component, Default)]
struct Indirect;

#[derive(Component, Default)]
#[require(Indirect)]
struct Parent;

#[derive(Component)]
#[require(Parent)]
struct Root;

fn query(_: Query<&Indirect>) {}

fn main() {
    World::new().spawn(Root);
}
