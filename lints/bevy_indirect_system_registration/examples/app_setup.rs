#![expect(dead_code)]
use bevy_app::{App, Plugin, Update};
use bevy_ecs::{resource::Resource, schedule::SystemSet};

fn system() {}

mod lifecycle {
    use super::*;

    pub(super) fn install(app: &mut App, enabled: bool) {
        if enabled {
            app.add_systems(Update, system);
        }
    }
}

#[derive(Resource, Default)]
struct Settings;

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
struct Gameplay;

fn configure(app: &mut App) {
    app.init_resource::<Settings>()
        .configure_sets(Update, Gameplay);
}

struct OtherApp;
impl OtherApp {
    fn add_systems(&mut self) {}
}

fn unrelated(_: &mut App) {
    OtherApp.add_systems();
    let _deferred = |app: &mut App| {
        app.add_systems(Update, system)
            .is_plugin_added::<GamePlugin>()
    };
}

struct GamePlugin;
impl GamePlugin {
    fn install(&self, app: &mut App) {
        App::add_systems(app, Update, system);
    }
}

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        lifecycle::install(app, true);
        self.install(app);
        configure(app);
        unrelated(app);
    }
}

fn main() {}
