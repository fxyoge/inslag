#![expect(dead_code)]
use bevy_ecs::prelude::*;

#[derive(Component)]
struct Aliased;
type Alias = Aliased;

#[derive(Component)]
struct Contained;
struct Container(Contained);

#[derive(Component)]
struct CustomTrait;
trait Integration {}
impl Integration for CustomTrait {}

#[derive(Component)]
struct Lookalike;
mod custom {
    pub struct With<T>(pub std::marker::PhantomData<T>);
}
fn custom(_: custom::With<Lookalike>) {}

#[derive(Component)]
#[component(on_add = hook)]
struct Hooked;

fn hook(_: bevy_ecs::world::DeferredWorld, _: bevy_ecs::lifecycle::HookContext) {}

fn query(_: Query<(&Aliased, &Contained, &CustomTrait, &Lookalike, &Hooked)>) {}

fn main() {}
