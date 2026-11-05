#![expect(dead_code)]
use bevy_ecs::{prelude::*, reflect::ReflectComponent};
use bevy_reflect::Reflect;
use bevy_scene::prelude::*;

#[derive(Component, Default, Clone)]
struct Registered;

#[derive(Component, Default, Clone)]
struct InlineScene;

#[derive(Component, Default, Reflect)]
#[reflect(Component)]
struct Reflected;

fn query(_: Query<(&Registered, &InlineScene, &Reflected)>) {}

fn register<T: Component + Default>() -> impl Fn(&mut World) {
    |world| {
        world.spawn(T::default());
    }
}

fn scene() -> impl Scene {
    bsn! { InlineScene }
}

fn main() {
    let _ = register::<Registered>();
    let _ = scene();
}
