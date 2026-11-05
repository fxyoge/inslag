#![expect(dead_code)]
use bevy_app::{App, Plugin, Update};
use bevy_ecs::{schedule::IntoScheduleConfigs, system::ScheduleSystem};

fn system() {}
fn configurations<T, M>(systems: T) -> impl IntoScheduleConfigs<ScheduleSystem, M>
where
    T: IntoScheduleConfigs<ScheduleSystem, M>,
{
    systems
}

struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, configurations(system));
    }
}
fn main() {}
