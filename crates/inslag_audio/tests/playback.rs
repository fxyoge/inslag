#![cfg(feature = "wav")]

use std::time::Duration;

use bevy_app::{App, TaskPoolPlugin, Update};
use bevy_asset::{AssetPlugin, AssetServer, Assets, LoadState};
use bevy_audio::{AudioPlayer, AudioPlugin, AudioSink, AudioSinkPlayback, AudioSource};
use bevy_ecs::prelude::*;
use bevy_time::{TimePlugin, TimeUpdateStrategy};
use inslag_audio::{AudioDiagnostic, EnhancedAudioPlugin};
use inslag_state_tree::{
    InstanceStatus, StateTreeDefinition, StateTreeInstance, StateTreePlugin, scxml,
};

mod support;

#[test]
fn native_playback_starts_and_cancels_or_reports_unavailable_output() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("sound.wav"),
        support::wave(Duration::from_millis(10)),
    )
    .unwrap();
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        AssetPlugin {
            file_path: directory.path().to_string_lossy().into_owned(),
            watch_for_changes_override: Some(false),
            use_asset_processor_override: Some(false),
            ..Default::default()
        },
        AudioPlugin::default(),
        StateTreePlugin::new(Update),
        EnhancedAudioPlugin::new(Update),
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    let audio = app
        .world()
        .resource::<AssetServer>()
        .load::<AudioSource>("sound.wav");
    for update in 0..4_096 {
        app.update();
        if app
            .world()
            .resource::<Assets<AudioSource>>()
            .contains(audio.id())
        {
            break;
        }
        let states = app
            .world()
            .resource::<AssetServer>()
            .get_load_states(audio.id());
        assert!(
            !matches!(states, Some((LoadState::Failed(_), _, _))) && update < 4_095,
            "audio did not load: {states:?}"
        );
        std::thread::yield_now();
    }
    let source = r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:ist="urn:inslag-state-tree" datamodel="luau" initial="active">
        <state id="active">
            <ist:activity id="sound" name="audio.playback">
                <ist:arg name="src" expr="'sound.wav'"/>
                <ist:arg name="loop" expr="true"/>
            </ist:activity>
            <transition event="error.activity.sound" target="done"/>
        </state>
        <final id="done"/>
    </scxml>"#;
    let definition = scxml::compile(&scxml::parse("playback.scxml", source).unwrap()).unwrap();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition);
    let owner = app.world_mut().spawn(StateTreeInstance::new(handle)).id();
    app.update();
    app.update();
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .status(),
        &InstanceStatus::Running
    );
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<AudioPlayer>>()
        .single(app.world())
        .unwrap();
    if let Some(sink) = app.world().get::<AudioSink>(player) {
        assert!(!sink.empty());
        app.world_mut().despawn(owner);
        app.update();
        assert!(
            app.world()
                .resource::<Messages<AudioDiagnostic>>()
                .is_empty()
        );
    } else {
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(3)));
        app.update();
        let failures: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<AudioDiagnostic>>()
            .drain()
            .collect();
        assert_eq!(failures.len(), 1);
        assert!(
            failures[0].reason.contains("device unavailable"),
            "{failures:?}"
        );
        app.update();
        assert_eq!(
            app.world()
                .get::<StateTreeInstance>(owner)
                .unwrap()
                .status(),
            &InstanceStatus::Complete
        );
    }
    assert!(app.world().get_entity(player).is_err());
    assert!(
        app.world_mut()
            .query::<&AudioPlayer>()
            .iter(app.world())
            .next()
            .is_none()
    );
}
