#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Component, Default)]
struct Generated;

#[derive(Component)]
struct ConsumerOnly;

macro_rules! construct {
    ($component:ty) => {
        <$component>::default()
    };
}

macro_rules! consumer {
    ($component:ty) => {
        fn generated_consumer(_: Query<&$component>) {}
    };
}

fn query(_: Query<&Generated>) {}
consumer!(ConsumerOnly);

fn main() {
    World::new().spawn(construct!(Generated));
}
