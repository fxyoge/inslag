#![allow(dead_code, clippy::type_complexity)]
use bevy_app::{App, Update};
use bevy_ecs::prelude::*;

#[derive(Component)]
struct Missing;

#[derive(Component, Default, Clone, Copy, Debug, PartialEq, Eq)]
struct UnusedDefault;

#[derive(Component)]
struct Excluded;

#[derive(Component)]
struct Optional;

#[derive(Component)]
struct Mutable {
    value: u32,
}

#[derive(Component)]
struct Alternative;

#[derive(Component)]
struct Tracked;

#[derive(Component)]
struct Checked;

fn required(query: Query<Entity, With<Missing>>) {
    for _ in &query {}
}

fn default_is_not_a_producer(query: Query<&UnusedDefault>) {
    for _ in query.iter() {}
}

fn excluded(query: Query<Entity, Without<Excluded>>) {
    let _ = query.is_empty();
}

fn optional(query: Query<(Entity, Option<&Optional>)>) {
    for _ in &query {}
}

fn mutable(mut query: Query<&mut Mutable>) {
    for mut value in &mut query {
        value.value += 1;
    }
}

fn alternative(query: Query<Entity, Or<(With<Alternative>, Without<Missing>)>>) {
    let _ = query.is_empty();
}

fn tracked(query: Query<Entity, (Added<Tracked>, Changed<Tracked>)>) {
    let _ = query.is_empty();
}

fn checked(query: Query<Has<Checked>>) {
    for _ in &query {}
}

fn main() {
    App::new().add_systems(Update, (required, mutable).chain());
}
