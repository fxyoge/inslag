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

fn setup(app: &mut App) {
    app.add_systems(Update, configurations());
}

struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, _: &mut App) {}
    fn finish(&self, app: &mut App) {
        setup(app);
        app.add_systems(Update, configurations());
    }
}
fn main() {}
