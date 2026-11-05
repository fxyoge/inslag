#![expect(dead_code)]
use bevy_app::{App, Update};
use bevy_ecs::{
    schedule::{IntoScheduleConfigs, ScheduleConfigs},
    system::ScheduleSystem,
};

fn system() {}
fn configurations() -> ScheduleConfigs<ScheduleSystem> {
    system.into_configs()
}

// An identically named trait and method are not Bevy's Plugin::build.
trait Plugin {
    fn build(&self, app: &mut App);
}
struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, configurations());
    }
}
struct Builder;
impl Builder {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, configurations());
    }
}
fn main() {}
