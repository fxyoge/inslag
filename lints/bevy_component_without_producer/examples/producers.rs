#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Component)]
struct Unit;
#[derive(Component)]
struct Tuple(u32);
#[derive(Component)]
struct Named {
    value: u32,
}
#[derive(Component, Default)]
struct Defaulted(u32);
#[derive(Component)]
enum Choice {
    First,
    Second(u32),
}

fn queries(_: Query<(&Unit, &Tuple, &Named, &Defaulted, &Choice)>) {}

mod producers {
    use super::*;

    pub(super) fn insert(world: &mut World) {
        world.spawn((
            Unit,
            Tuple(1),
            Named { value: 2 },
            Defaulted::default(),
            Choice::First,
        ));
    }
}

fn main() {}
