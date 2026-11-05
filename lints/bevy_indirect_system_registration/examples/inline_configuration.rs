#![expect(dead_code)]
use bevy_app::{App, Plugin, Update};
use bevy_ecs::schedule::IntoScheduleConfigs;

fn first() {}
fn second() {}
fn enabled() -> bool {
    true
}

struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (first, second).chain().run_if(enabled));
        app.add_systems(Update, || {});
    }
}
fn main() {}
