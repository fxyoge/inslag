use super::*;

use bevy_app::{TaskPoolPlugin, Update};
use bevy_asset::{AssetApp, AssetPlugin};
use bevy_time::{TimePlugin, TimeUpdateStrategy};
use inslag_state_tree::{InstanceStatus, StateTreeDefinition, StateTreePlugin};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        AssetPlugin::default(),
        bevy_audio::AudioPlugin::default(),
    ))
    .init_asset::<AudioSource>()
    .add_plugins((
        StateTreePlugin::new(Update),
        crate::EnhancedAudioPlugin::new(Update),
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )));
    app.world_mut()
        .resource_mut::<Time<bevy_time::Virtual>>()
        .set_max_delta(Duration::from_secs(1_000));
    app.world_mut().resource_mut::<Backend>().simulation = Some(Simulation::default());
    app
}

fn chart(app: &mut App, body: &str) -> Entity {
    let source = format!(
        r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:ist="urn:inslag-state-tree" datamodel="luau" initial="active">{body}</scxml>"#
    );
    let parsed = inslag_state_tree::scxml::parse("audio-test.scxml", &source).unwrap();
    let definition = inslag_state_tree::scxml::compile(&parsed).unwrap();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition);
    app.world_mut().spawn(StateTreeInstance::new(handle)).id()
}

fn source(app: &mut App, src: &str, result: Result<Option<Duration>, &str>) {
    app.world_mut()
        .resource_mut::<Backend>()
        .simulation
        .as_mut()
        .unwrap()
        .sources
        .insert(src.into(), result.map_err(str::to_owned));
}

fn play(src: &str, extra: &str) -> String {
    format!(
        r#"<ist:effect name="audio.play"><ist:arg name="src" expr="'{src}'"/>{extra}</ist:effect>"#
    )
}

fn activity(src: &str, extra: &str) -> String {
    format!(
        r#"<ist:activity id="sound" name="audio.playback"><ist:arg name="src" expr="'{src}'"/>{extra}</ist:activity>"#
    )
}

fn advance_time(app: &mut App, duration: Duration) {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(duration));
    app.update();
}

fn paths(app: &App) -> Vec<String> {
    app.world()
        .resource::<Runtime>()
        .voices
        .iter()
        .map(|voice| voice.request.sound.src.to_string())
        .collect()
}

#[test]
fn one_shot_uses_authored_asset_and_survives_state_exit() {
    let mut app = app();
    source(&mut app, "confirm.wav", Ok(Some(Duration::from_secs(1))));
    let entity = chart(
        &mut app,
        &format!(
            r#"<state id="active"><onentry>{}</onentry><transition target="done"/></state><final id="done"/>"#,
            play("confirm.wav", "")
        ),
    );
    app.update();
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .status(),
        &InstanceStatus::Complete
    );
    assert_eq!(paths(&app), ["confirm.wav"]);
    advance_time(&mut app, Duration::from_secs(2));
    assert!(paths(&app).is_empty());
}

#[test]
fn finite_activity_completes_through_normal_state_tree_outcomes() {
    let mut app = app();
    source(&mut app, "finite.wav", Ok(Some(Duration::from_millis(10))));
    let owner = chart(
        &mut app,
        &format!(
            r#"<state id="active">{}<transition event="done.activity.sound" target="done"/></state><final id="done"/>"#,
            activity("finite.wav", "")
        ),
    );
    app.update();
    app.update();
    app.update();
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .status(),
        &InstanceStatus::Complete
    );
    assert!(paths(&app).is_empty());
}

#[test]
fn activity_load_failure_is_reported_once() {
    let mut app = app();
    source(&mut app, "bad.wav", Err("missing file"));
    let owner = chart(
        &mut app,
        &format!(
            r#"<state id="active">{}<transition event="error.activity.sound" target="failed"/></state><final id="failed"/>"#,
            activity("bad.wav", "")
        ),
    );
    app.update();
    assert_eq!(app.world().resource::<Messages<AudioDiagnostic>>().len(), 1);
    app.update();
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .status(),
        &InstanceStatus::Complete
    );
    assert!(paths(&app).is_empty());
}

#[test]
fn cancellation_during_loading_and_same_frame_start_cancel_never_play() {
    for immediate in [false, true] {
        let mut app = app();
        source(&mut app, "loading.wav", Ok(None));
        let transition = if immediate {
            r#"<transition target="done"/>"#
        } else {
            r#"<transition ist:after="1s" target="done"/>"#
        };
        chart(
            &mut app,
            &format!(
                r#"<state id="active">{}{transition}</state><final id="done"/>"#,
                activity("loading.wav", "")
            ),
        );
        app.update();
        if !immediate {
            assert_eq!(paths(&app), ["loading.wav"]);
        }
        advance_time(&mut app, Duration::from_secs(1));
        source(&mut app, "loading.wav", Ok(Some(Duration::from_secs(1))));
        app.update();
        assert!(paths(&app).is_empty());
        assert!(
            app.world()
                .resource::<Backend>()
                .simulation
                .as_ref()
                .unwrap()
                .starts
                .is_empty()
        );
    }
}

#[test]
fn looping_activity_is_owned_and_despawn_stops_it() {
    let mut app = app();
    source(&mut app, "music.wav", Ok(Some(Duration::from_millis(1))));
    let owner = chart(
        &mut app,
        &format!(
            r#"<state id="active">{}</state>"#,
            activity("music.wav", r#"<ist:arg name="loop" expr="true"/>"#)
        ),
    );
    app.update();
    advance_time(&mut app, Duration::from_secs(100));
    assert_eq!(paths(&app), ["music.wav"]);
    let player = app.world().resource::<Runtime>().voices[0]
        .entity()
        .unwrap();
    app.world_mut().despawn(owner);
    app.update();
    assert!(paths(&app).is_empty());
    assert!(app.world().get_entity(player).is_err());
}

#[test]
fn preference_and_authored_gains_multiply_and_mute_pending_and_active_playback() {
    let mut app = app();
    source(&mut app, "music.wav", Ok(Some(Duration::from_secs(10))));
    app.world_mut()
        .resource_mut::<AudioMixer>()
        .set_preference("music", 0.5)
        .unwrap();
    chart(
        &mut app,
        &format!(
            r#"<state id="active"><onentry><ist:effect name="audio.set_bus_gain"><ist:arg name="bus" expr="'music'"/><ist:arg name="gain" expr="0.4"/></ist:effect></onentry>{}</state>"#,
            activity(
                "music.wav",
                r#"<ist:arg name="bus" expr="'music'"/><ist:arg name="gain" expr="0.5"/>"#
            )
        ),
    );
    app.update();
    let player = app.world().resource::<Runtime>().voices[0]
        .entity()
        .unwrap();
    assert_eq!(
        app.world().get::<PlaybackSettings>(player).unwrap().volume,
        Volume::Linear(0.1)
    );
    app.world_mut()
        .resource_mut::<AudioMixer>()
        .set_preference("music", 0.0)
        .unwrap();
    app.update();
    assert_eq!(
        app.world().get::<PlaybackSettings>(player).unwrap().volume,
        Volume::Linear(0.0)
    );
    app.world_mut()
        .resource_mut::<AudioMixer>()
        .set_preference("music", 1.0)
        .unwrap();
    app.update();
    assert_eq!(
        app.world().get::<PlaybackSettings>(player).unwrap().volume,
        Volume::Linear(0.2)
    );
}

#[test]
fn burst_budget_preserves_priority_and_does_not_steal_activity_voices() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<AudioSettings>()
        .max_one_shots = 2;
    for name in ["music.wav", "low.wav", "high.wav"] {
        source(&mut app, name, Ok(Some(Duration::from_secs(10))));
    }
    let low = play("low.wav", "");
    let high = play("high.wav", r#"<ist:arg name="priority" expr="10"/>"#);
    chart(
        &mut app,
        &format!(
            r#"<state id="active">{}<onentry>{}{high}</onentry></state>"#,
            activity("music.wav", ""),
            low.repeat(100)
        ),
    );
    app.update();
    let paths = paths(&app);
    assert_eq!(paths.len(), 3);
    assert_eq!(paths.iter().filter(|path| *path == "high.wav").count(), 1);
    assert_eq!(paths.iter().filter(|path| *path == "music.wav").count(), 1);
}

#[test]
fn concurrency_is_scoped_to_owner_and_authored_group() {
    let mut app = app();
    source(&mut app, "move.wav", Ok(Some(Duration::from_secs(1))));
    let requests = play("move.wav", r#"<ist:arg name="group" expr="'movement'"/>"#).repeat(20);
    for _ in 0..2 {
        chart(
            &mut app,
            &format!(r#"<state id="active"><onentry>{requests}</onentry></state>"#),
        );
    }
    app.update();
    assert_eq!(paths(&app).len(), 2);
}

#[test]
fn loading_and_cache_are_bounded_and_stale_one_shots_expire() {
    let mut app = app();
    {
        let mut settings = app.world_mut().resource_mut::<AudioSettings>();
        settings.max_pending = 3;
        settings.cache_capacity = 2;
    }
    let requests: String = (0..20)
        .map(|index| play(&format!("{index}.wav"), ""))
        .collect();
    chart(
        &mut app,
        &format!(r#"<state id="active"><onentry>{requests}</onentry></state>"#),
    );
    app.update();
    assert_eq!(paths(&app).len(), 3);
    assert_eq!(app.world().resource::<Runtime>().cache.len(), 2);
    advance_time(&mut app, Duration::from_secs(1));
    assert!(paths(&app).is_empty());
}

#[test]
fn invalid_arguments_fault_instead_of_becoming_playback_requests() {
    for extra in [
        r#"<ist:arg name="speed" expr="0"/>"#,
        r#"<ist:arg name="gain" expr="2"/>"#,
        r#"<ist:arg name="limit" expr="0"/>"#,
        r#"<ist:arg name="loop" expr="true"/>"#,
    ] {
        let mut app = app();
        let owner = chart(
            &mut app,
            &format!(
                r#"<state id="active"><onentry>{}</onentry></state>"#,
                play("sound.wav", extra)
            ),
        );
        app.update();
        assert!(matches!(
            app.world()
                .get::<StateTreeInstance>(owner)
                .unwrap()
                .status(),
            InstanceStatus::Faulted(_)
        ));
        assert!(paths(&app).is_empty());
    }
}

#[test]
fn mixer_rejects_invalid_values_and_preserves_registered_preferences() {
    let mut mixer = AudioMixer::default();
    for value in [-1.0, f32::NAN, f32::INFINITY, 1.1] {
        assert!(mixer.set_preference("master", value).is_err());
    }
    assert!(mixer.set_preference("missing", 1.0).is_err());
    mixer.register_bus("ambience").unwrap();
    mixer.set_preference("ambience", 0.25).unwrap();
    mixer.register_bus("ambience").unwrap();
    assert_eq!(mixer.gain("ambience"), Ok(0.25));
}

#[test]
fn loading_activity_times_out_in_real_time() {
    let mut app = app();
    let owner = chart(
        &mut app,
        &format!(
            r#"<state id="active">{}<transition event="error.activity.sound" target="failed"/></state><final id="failed"/>"#,
            activity("loading.wav", "")
        ),
    );
    app.update();
    advance_time(&mut app, Duration::from_secs(11));
    app.update();
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .status(),
        &InstanceStatus::Complete
    );
    assert!(paths(&app).is_empty());
}

#[test]
fn reentry_replaces_generation_without_stopping_the_new_run() {
    let mut app = app();
    source(&mut app, "loop.wav", Ok(Some(Duration::from_secs(1))));
    let owner = chart(
        &mut app,
        &format!(
            r#"<state id="active">{}<transition ist:after="1s" target="active"/></state>"#,
            activity("loop.wav", r#"<ist:arg name="loop" expr="true"/>"#)
        ),
    );
    app.update();
    let old = app.world().resource::<Runtime>().voices[0]
        .request
        .token
        .unwrap();
    let old_entity = app.world().resource::<Runtime>().voices[0]
        .entity()
        .unwrap();
    advance_time(&mut app, Duration::from_secs(1));
    let runtime = app.world().resource::<Runtime>();
    assert_eq!(runtime.voices.len(), 1);
    assert_ne!(runtime.voices[0].request.token, Some(old));
    assert!(
        app.world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .is_activity_active(runtime.voices[0].request.token.unwrap())
    );
    assert!(app.world().get_entity(old_entity).is_err());
    assert_eq!(
        app.world()
            .resource::<Backend>()
            .simulation
            .as_ref()
            .unwrap()
            .starts
            .len(),
        1
    );
}

#[test]
fn chart_replacement_cancels_old_playback() {
    let mut app = app();
    source(&mut app, "loop.wav", Ok(Some(Duration::from_secs(1))));
    let owner = chart(
        &mut app,
        &format!(
            r#"<state id="active">{}</state>"#,
            activity("loop.wav", r#"<ist:arg name="loop" expr="true"/>"#)
        ),
    );
    app.update();
    let replacement = chart(&mut app, r#"<state id="active"/>"#);
    let definition = app
        .world()
        .get::<StateTreeInstance>(replacement)
        .unwrap()
        .definition()
        .clone();
    app.world_mut()
        .entity_mut(owner)
        .insert(StateTreeInstance::new(definition));
    app.update();
    assert!(paths(&app).is_empty());
}

#[test]
fn non_update_schedule_consumes_requests_in_its_own_run() {
    #[derive(Clone, Debug, PartialEq, Eq, Hash, bevy_ecs::schedule::ScheduleLabel)]
    struct ChartUpdate;
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        AssetPlugin::default(),
        bevy_audio::AudioPlugin::default(),
    ))
    .init_asset::<AudioSource>()
    .add_plugins((
        StateTreePlugin::new(ChartUpdate),
        crate::EnhancedAudioPlugin::new(ChartUpdate),
    ));
    app.world_mut().resource_mut::<Backend>().simulation = Some(Simulation::default());
    source(&mut app, "custom.wav", Ok(Some(Duration::from_secs(1))));
    chart(
        &mut app,
        &format!(
            r#"<state id="active"><onentry>{}</onentry></state>"#,
            play("custom.wav", "")
        ),
    );
    app.world_mut().run_schedule(ChartUpdate);
    assert_eq!(paths(&app), ["custom.wav"]);
}

#[cfg(feature = "wav")]
fn wave() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&196_u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&8_000_u32.to_le_bytes());
    bytes.extend_from_slice(&16_000_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&160_u32.to_le_bytes());
    bytes.resize(204, 0);
    bytes
}

#[test]
#[cfg(feature = "wav")]
fn actual_bevy_backend_loads_and_finishes_or_reports_unavailable_output() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("sound.wav"), wave()).unwrap();
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        AssetPlugin {
            file_path: directory.path().to_string_lossy().into_owned(),
            ..Default::default()
        },
        bevy_audio::AudioPlugin::default(),
        StateTreePlugin::new(Update),
        crate::EnhancedAudioPlugin::new(Update),
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )))
    .insert_resource(AudioSettings {
        load_timeout: Duration::from_secs(1_000),
        ..Default::default()
    });
    let owner = chart(
        &mut app,
        &format!(
            r#"<state id="active">{}<transition event="done.activity.sound error.activity.sound" target="done"/></state><final id="done"/>"#,
            activity("sound.wav", "")
        ),
    );
    let mut failures = Vec::new();
    for _ in 0..4_096 {
        app.update();
        failures.extend(
            app.world_mut()
                .resource_mut::<Messages<AudioDiagnostic>>()
                .drain(),
        );
        if app
            .world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .status()
            == &InstanceStatus::Complete
        {
            assert!(paths(&app).is_empty());
            assert!(
                failures
                    .iter()
                    .all(|failure| failure.reason.contains("device unavailable")),
                "{failures:?}"
            );
            assert!(
                app.world()
                    .resource::<Runtime>()
                    .cache
                    .values()
                    .any(|entry| entry
                        .validation
                        .as_ref()
                        .is_some_and(|validation| validation.result.is_ok()))
            );
            return;
        }
        std::thread::yield_now();
    }
    panic!("actual audio playback never settled");
}

#[test]
#[cfg(feature = "wav")]
fn corrupt_audio_is_rejected_before_bevys_decoder_can_panic() {
    let mut runtime = Runtime::default();
    let path = AssetPath::from("bad.wav").into_owned();
    let invalid = AudioSource {
        bytes: vec![0, 1, 2].into(),
    };
    assert!(runtime.validate(&path, &invalid).is_err());
    let valid = AudioSource {
        bytes: wave().into(),
    };
    assert!(runtime.validate(&path, &valid).is_ok());
}
