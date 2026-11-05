#![expect(dead_code)]
use bevy_app::{App, Plugin, Update};
use bevy_ecs::{
    schedule::{IntoScheduleConfigs, ScheduleConfigs},
    system::ScheduleSystem,
};

fn system() {}
fn configurations() -> ScheduleConfigs<ScheduleSystem> {
    system.into_configs()
}

struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        let systems = configurations();
        app.add_systems(Update, systems);
    }
}
fn main() {}
