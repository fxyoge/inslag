use bevy::prelude::*;
use bevy_flair::FlairPlugin;
use inslag_kui::state_tree::KuiStateTreePlugin;
use inslag_state_tree::{StateTreeDefinition, StateTreeInstance, StateTreePlugin};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(AssetPlugin {
                file_path: format!("{}/assets", env!("CARGO_MANIFEST_DIR")),
                ..default()
            }),
            FlairPlugin,
            StateTreePlugin::new(Update),
            KuiStateTreePlugin,
        ))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn(Camera2d);
    commands.spawn(StateTreeInstance::new(
        assets.load::<StateTreeDefinition>("example.scxml"),
    ));
}
