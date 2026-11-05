#![expect(dead_code)]
use bevy_app::{App, Plugin};

struct ScheduleSystem;
struct ScheduleConfigs<T>(T);
trait IntoScheduleConfigs<T, M> {}
impl IntoScheduleConfigs<ScheduleSystem, ()> for ScheduleSystem {}

fn configurations() -> ScheduleConfigs<ScheduleSystem> {
    ScheduleConfigs(ScheduleSystem)
}
fn opaque() -> impl IntoScheduleConfigs<ScheduleSystem, ()> {
    ScheduleSystem
}

struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, _: &mut App) {
        let _ = configurations();
        let _ = opaque();
    }
}
fn main() {}
