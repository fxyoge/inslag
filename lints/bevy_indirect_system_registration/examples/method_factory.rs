#![expect(dead_code)]
use bevy_app::{App, Plugin, Update};
use bevy_ecs::{
    schedule::{IntoScheduleConfigs, ScheduleConfigs},
    system::ScheduleSystem,
};

fn system() {}
struct GamePlugin;
impl GamePlugin {
    fn configurations(&self) -> ScheduleConfigs<ScheduleSystem> {
        system.into_configs()
    }
}
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, self.configurations());
    }
}
fn main() {}
