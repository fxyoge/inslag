#![expect(dead_code)]
use bevy_app::{App, Plugin, Update};
use bevy_ecs::{
    schedule::{IntoScheduleConfigs, ScheduleConfigs},
    system::ScheduleSystem,
};

type Configurations = ScheduleConfigs<ScheduleSystem>;
fn system() {}
fn configurations() -> Configurations {
    system.into_configs()
}

struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, configurations());
    }
}
fn main() {}
