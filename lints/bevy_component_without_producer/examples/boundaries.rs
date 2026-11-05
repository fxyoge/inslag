#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Component)]
pub struct Public;

#[derive(Component)]
struct Generic<T: Send + Sync + 'static>(T);

#[derive(Component)]
struct NotQueried;

#[derive(Component)]
#[cfg_attr(
    dylint_lib = "bevy_component_without_producer",
    allow(bevy_component_without_producer)
)]
struct Allowed;

#[derive(Component)]
#[cfg_attr(
    dylint_lib = "bevy_component_without_producer",
    expect(bevy_component_without_producer)
)]
struct Expected;

fn query(_: Query<(&Public, &Generic<u32>, &Allowed, &Expected)>) {}

fn main() {}
