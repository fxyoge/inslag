#![expect(dead_code)]
use bevy_app::{App, Plugin, Update};
use bevy_ecs::{schedule::IntoScheduleConfigs, system::ScheduleSystem};

fn system() {}
fn configurations() -> impl IntoScheduleConfigs<ScheduleSystem, ()> {
    system.into_configs()
}

struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, configurations());
    }
}
fn main() {}
