use bevy::prelude::*;
use inslag_kui::state_tree::KuiStateTreePlugin;
use inslag_state_tree::{StateTreeInstance, StateTreePlugin};

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(AssetPlugin {
                file_path: format!("{}/assets", env!("CARGO_MANIFEST_DIR")),
                ..default()
            }),
            bevy_flair::FlairPlugin,
            StateTreePlugin::new(Update),
            KuiStateTreePlugin,
        ))
        .add_systems(
            Startup,
            |mut commands: Commands, assets: Res<AssetServer>| {
                commands.spawn(Camera2d);
                commands.spawn(StateTreeInstance::new(
                    assets.load("component-showcase.scxml"),
                ));
            },
        )
        .run();
}
