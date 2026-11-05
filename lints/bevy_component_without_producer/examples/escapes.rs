#![expect(dead_code, private_interfaces)]
use bevy_ecs::prelude::*;

#[derive(Component, Default)]
struct Factory(u32);
#[derive(Component)]
struct SystemEscape;
#[derive(Component)]
struct QueryEscape;
#[derive(Component)]
struct Unknown;
#[derive(Component)]
struct Captured(u32);

fn query(_: Query<(&Factory, &Captured)>) {}
fn system(_: Query<&SystemEscape>) {}

pub fn factory() -> impl Component {
    Factory::default()
}

pub fn system_factory() -> impl Fn(Query<&SystemEscape>) {
    system
}

pub fn query_escape(_: Query<&QueryEscape>) {}

fn unknown<T>(_: T) {}

fn pass_query(query: Query<&Unknown>) {
    unknown(query);
}

fn capture_constructor() {
    let _ = Captured;
}

fn main() {}
