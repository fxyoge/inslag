//! Click, hover and controller-authored dragging use the same action protocol.
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
        .insert_resource(ClearColor(Color::srgb(0.08, 0.10, 0.13)))
        .add_systems(
            Startup,
            |mut commands: Commands, assets: Res<AssetServer>| {
                commands.spawn(Camera2d);
                commands.spawn((
                    Transform::default(),
                    Visibility::default(),
                    StateTreeInstance::new(assets.load("interactions.scxml")),
                ));
            },
        )
        .run();
}
