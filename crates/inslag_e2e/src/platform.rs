//! Synthetic platform; gameplay composition is supplied by the consumer.
use bevy::{asset::AssetPlugin, camera::CameraUpdateSystems, prelude::*, time::TimeUpdateStrategy};
use std::{path::Path, time::Duration};

pub(crate) fn app(dt: Duration, assets: &Path, viewport: UVec2) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: assets.to_string_lossy().into_owned(),
            watch_for_changes_override: Some(false),
            use_asset_processor_override: Some(false),
            ..default()
        },
    ));
    app.init_asset::<bevy::shader::Shader>()
        .register_asset_loader(bevy::shader::ShaderLoader);
    app.add_plugins(
        DefaultPlugins
            .build()
            .set(ImagePlugin::default_nearest())
            .disable::<bevy::app::TaskPoolPlugin>()
            .disable::<bevy::time::TimePlugin>()
            .disable::<bevy::diagnostic::FrameCountPlugin>()
            .disable::<AssetPlugin>()
            .disable::<bevy::log::LogPlugin>()
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::render::RenderPlugin>()
            .set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: (viewport.x, viewport.y).into(),
                    ..default()
                }),
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            }),
    );
    app.add_message::<bevy::window::WindowEvent>()
        .add_plugins(bevy::render::texture::TexturePlugin)
        .register_asset_loader(bevy::image::ImageLoader::new(
            bevy::image::CompressedImageFormats::empty(),
        ))
        .add_systems(
            PostStartup,
            bevy::render::camera::camera_system.in_set(CameraUpdateSystems),
        )
        .add_systems(
            PostUpdate,
            bevy::render::camera::camera_system
                .in_set(CameraUpdateSystems)
                .before(bevy::camera::visibility::update_frusta),
        )
        .insert_resource(TimeUpdateStrategy::ManualDuration(dt));
    app
}

#[cfg(test)]
mod tests {
    use super::*;
    use inslag_audio::EnhancedAudioPlugin;
    use inslag_state_tree::StateTreePlugin;

    #[test]
    fn production_audio_composition_builds_and_updates_headlessly() {
        let _serial = crate::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let assets = tempfile::tempdir().unwrap();
        let mut app = app(
            Duration::from_secs_f64(1.0 / 60.0),
            assets.path(),
            UVec2::new(640, 480),
        );
        app.add_plugins((
            StateTreePlugin::new(Update),
            EnhancedAudioPlugin::new(Update),
        ));
        app.finish();
        app.cleanup();
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().contains_resource::<Assets<AudioSource>>());
        assert!(!app.is_plugin_added::<bevy::render::RenderPlugin>());
    }
}
