#![expect(dead_code)]
use bevy_app::{App, Plugin, Update};
use bevy_ecs::schedule::{InternedSystemSet, IntoScheduleConfigs, ScheduleConfigs, SystemSet};

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
enum Sets {
    First,
    Second,
}

fn sets() -> ScheduleConfigs<InternedSystemSet> {
    (Sets::First, Sets::Second).chain()
}
fn opaque_sets() -> impl IntoScheduleConfigs<InternedSystemSet, ()> {
    (Sets::First, Sets::Second).chain()
}

struct GamePlugin;
impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(Update, sets());
        app.configure_sets(Update, opaque_sets());
    }
}
fn main() {}
