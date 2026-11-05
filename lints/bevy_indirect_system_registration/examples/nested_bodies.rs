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
        // Runtime system bodies are not the plugin's build method.
        app.add_systems(Update, || {
            let _ = configurations();
        });
        // A nested helper body is not inspected transitively either.
        fn setup(app: &mut App) {
            app.add_systems(Update, configurations());
        }
        setup(app);
    }
}
fn main() {}
