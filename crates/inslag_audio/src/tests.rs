use super::*;

use std::{path::Path, sync::Mutex};

use bevy_app::{PostUpdate, PreUpdate, TaskPoolPlugin, Update};
use bevy_asset::{
    AssetApp, AssetPlugin,
    io::{
        AssetSourceBuilder, AssetSourceId,
        memory::{Dir, MemoryAssetReader},
    },
};
use bevy_audio::{AudioLoader, Decodable, PlaybackMode};
use bevy_ecs::schedule::ScheduleLabel;
use bevy_time::{TimePlugin, TimeUpdateStrategy};
use inslag_state_tree::{InstanceStatus, StateTreeDefinition, StateTreePlugin};
use rodio::Source;

#[path = "../tests/support/mod.rs"]
mod support;
use support::wave;
#[path = "../tests/support/gated_reader.rs"]
mod gated_reader;
use gated_reader::{GateOpener, GatedReader};

#[derive(Resource)]
struct Sources {
    directory: Dir,
    gates: GateOpener,
    handles: HashMap<String, Handle<AudioSource>>,
}

impl Drop for Sources {
    fn drop(&mut self) {
        self.gates.open_all();
    }
}

#[derive(Component)]
struct OfflineOutput(Mutex<rodio::queue::SourcesQueueOutput>);

fn attach_sinks(
    players: Query<(Entity, &AudioPlayer, &PlaybackSettings), Without<AudioSink>>,
    sources: Res<Assets<AudioSource>>,
    global: Res<GlobalVolume>,
    mut commands: Commands,
) {
    for (entity, player, settings) in &players {
        let source = sources.get(&player.0).unwrap();
        let (player, output) = rodio::Player::new();
        player.set_volume((settings.volume * global.volume).to_linear());
        player.set_speed(settings.speed);
        if matches!(settings.mode, PlaybackMode::Loop) {
            player.append(source.decoder().repeat_infinite());
        } else {
            player.append(source.decoder());
        }
        commands
            .entity(entity)
            .insert((AudioSink::new(player), OfflineOutput(Mutex::new(output))));
    }
}

fn drain_sinks(time: Res<Time<Real>>, mut outputs: Query<&mut OfflineOutput>) {
    for mut output in &mut outputs {
        output
            .0
            .get_mut()
            .unwrap()
            .take_duration(time.delta())
            .for_each(drop);
    }
}

fn app() -> App {
    app_with_schedule(Update)
}

fn app_with_schedule(schedule: impl ScheduleLabel) -> App {
    let schedule = schedule.intern();
    let directory = Dir::default();
    let (reader, gates) = GatedReader::new(MemoryAssetReader {
        root: directory.clone(),
    });
    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(reader.clone())),
    );
    app.add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        AssetPlugin {
            watch_for_changes_override: Some(false),
            use_asset_processor_override: Some(false),
            ..Default::default()
        },
        StateTreePlugin::new(schedule),
    ))
    .init_asset::<AudioSource>()
    .register_asset_loader(AudioLoader)
    .init_resource::<GlobalVolume>()
    .init_resource::<AudioSettings>()
    .init_resource::<AudioMixer>()
    .init_resource::<Runtime>()
    .insert_resource(Sources {
        directory,
        gates,
        handles: HashMap::new(),
    })
    .add_message::<AudioDiagnostic>()
    .add_state_tree_effect::<Play>("audio.play")
    .add_state_tree_activity(
        inslag_state_tree::Activity::<Playback>::new("audio.playback").with_lifecycle(),
    )
    .add_state_tree_effect::<BusGain>("audio.set_bus_gain")
    .add_systems(
        schedule,
        collect_activities.in_set(StateTreeSet::HandleActivities),
    )
    .add_systems(
        schedule,
        collect_effects.in_set(StateTreeSet::HandleEffects),
    )
    .add_systems(schedule, advance.after(StateTreeSet::HandleEffects))
    .add_systems(PreUpdate, drain_sinks)
    .add_systems(PostUpdate, attach_sinks)
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        16,
    )));
    app.world_mut()
        .resource_mut::<Time<bevy_time::Virtual>>()
        .set_max_delta(Duration::from_secs(1_000));
    app.update();
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

fn source(app: &mut App, src: &str, duration: Duration) {
    let handle = app.world().resource::<AssetServer>().load(src.to_owned());
    app.world_mut()
        .resource_mut::<Sources>()
        .handles
        .insert(src.into(), handle.clone());
    let sources = app.world().resource::<Sources>();
    sources
        .directory
        .insert_asset(Path::new(src), wave(duration));
    sources.gates.open(src);
    wait_for(app, |app| {
        let server = app.world().resource::<AssetServer>();
        assert!(
            !matches!(
                server.get_load_state(handle.id()),
                Some(LoadState::Failed(_))
            ),
            "{src}: {:?}",
            server.get_load_states(handle.id())
        );
        app.world()
            .resource::<Assets<AudioSource>>()
            .contains(handle.id())
    });
}

#[track_caller]
fn wait_for(app: &mut App, predicate: impl Fn(&App) -> bool) {
    let strategy = app
        .world_mut()
        .remove_resource::<TimeUpdateStrategy>()
        .unwrap();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    for _ in 0..4_096 {
        app.update();
        if predicate(app) {
            app.insert_resource(strategy);
            return;
        }
        std::thread::yield_now();
    }
    app.insert_resource(strategy);
    panic!("audio did not settle after 4096 updates");
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
fn offline_playback_does_not_install_the_native_audio_plugin() {
    let app = app();
    assert!(!app.is_plugin_added::<AudioPlugin>());
}

#[test]
#[should_panic(expected = "install Bevy's AudioPlugin before EnhancedAudioPlugin")]
fn real_playback_requires_the_native_audio_plugin() {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        AssetPlugin::default(),
        StateTreePlugin::new(Update),
    ))
    .init_asset::<AudioSource>()
    .add_plugins(EnhancedAudioPlugin::new(Update));
}

#[test]
fn one_shot_uses_authored_asset_and_survives_state_exit() {
    let mut app = app();
    source(&mut app, "confirm.wav", Duration::from_secs(1));
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
    source(&mut app, "finite.wav", Duration::from_millis(10));
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
    app.world().resource::<Sources>().gates.open("bad.wav");
    let owner = chart(
        &mut app,
        &format!(
            r#"<state id="active">{}<transition event="error.activity.sound" target="failed"/></state><final id="failed"/>"#,
            activity("bad.wav", "")
        ),
    );
    wait_for(&mut app, |app| {
        app.world().resource::<Messages<AudioDiagnostic>>().len() == 1
    });
    assert!(
        app.world()
            .resource::<Messages<AudioDiagnostic>>()
            .iter_current_update_messages()
            .next()
            .unwrap()
            .reason
            .contains("bad.wav")
    );
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
        let transition = if immediate {
            r#"<transition target="done"/>"#
        } else {
            r#"<transition ist:after="1" target="done"/>"#
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
        source(&mut app, "loading.wav", Duration::from_secs(1));
        app.update();
        assert!(paths(&app).is_empty());
        assert!(
            app.world_mut()
                .query::<&AudioPlayer>()
                .iter(app.world())
                .next()
                .is_none()
        );
    }
}

#[test]
fn looping_activity_is_owned_and_despawn_stops_it() {
    let mut app = app();
    source(&mut app, "music.wav", Duration::from_millis(1));
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
    source(&mut app, "music.wav", Duration::from_secs(10));
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
    assert_eq!(
        app.world().get::<AudioSink>(player).unwrap().volume(),
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
    assert_eq!(
        app.world().get::<AudioSink>(player).unwrap().volume(),
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
    assert_eq!(
        app.world().get::<AudioSink>(player).unwrap().volume(),
        Volume::Linear(0.2)
    );
    app.world_mut().resource_mut::<GlobalVolume>().volume = Volume::Linear(0.5);
    app.update();
    assert_eq!(
        app.world().get::<AudioSink>(player).unwrap().volume(),
        Volume::Linear(0.1)
    );
}

#[test]
fn burst_budget_preserves_priority_and_does_not_steal_activity_voices() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<AudioSettings>()
        .max_one_shots = 2;
    for name in ["music.wav", "low.wav", "high.wav"] {
        source(&mut app, name, Duration::from_secs(10));
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
    source(&mut app, "move.wav", Duration::from_secs(1));
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
fn playback_values_are_not_domain_admitted_during_decoding() {
    for extra in [
        r#"<ist:arg name="speed" expr="0"/>"#,
        r#"<ist:arg name="gain" expr="2"/>"#,
        r#"<ist:arg name="limit" expr="0"/>"#,
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
        assert_eq!(
            app.world()
                .get::<StateTreeInstance>(owner)
                .unwrap()
                .status(),
            &InstanceStatus::Running
        );
    }
}

#[test]
fn backend_speed_failure_is_diagnosed_without_faulting_the_chart() {
    let mut app = app();
    let owner = chart(
        &mut app,
        &format!(
            r#"<state id="active"><onentry>{}</onentry></state>"#,
            play("sound.wav", r#"<ist:arg name="speed" expr="0"/>"#),
        ),
    );
    app.update();
    let tree = app.world().get::<StateTreeInstance>(owner).unwrap();
    assert_eq!(tree.status(), &InstanceStatus::Running);
    let diagnostics = app.world().resource::<Messages<AudioDiagnostic>>();
    let mut cursor = diagnostics.get_cursor();
    let diagnostic = cursor.read(diagnostics).next().unwrap();
    assert_eq!(diagnostic.owner, owner);
    assert!(diagnostic.reason.contains("speed"));
    assert!(paths(&app).is_empty());
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
    source(&mut app, "loop.wav", Duration::from_secs(1));
    let owner = chart(
        &mut app,
        &format!(
            r#"<state id="active">{}<transition ist:after="1" target="active"/></state>"#,
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
        app.world_mut()
            .query::<&OfflineOutput>()
            .iter(app.world())
            .count(),
        1
    );
}

#[test]
fn chart_replacement_cancels_old_playback() {
    let mut app = app();
    source(&mut app, "loop.wav", Duration::from_secs(1));
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
    let mut app = app_with_schedule(ChartUpdate);
    source(&mut app, "custom.wav", Duration::from_secs(1));
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

#[test]
fn corrupt_audio_is_rejected_before_bevys_decoder_can_panic() {
    let mut runtime = Runtime::default();
    let path = AssetPath::from("bad.wav").into_owned();
    let invalid = AudioSource {
        bytes: vec![0, 1, 2].into(),
    };
    assert!(runtime.validate(&path, &invalid).is_err());
    let valid = AudioSource {
        bytes: wave(Duration::from_millis(10)).into(),
    };
    assert!(runtime.validate(&path, &valid).is_ok());
}

#[test]
fn corrupt_sources_fail_before_starting_and_after_replacement() {
    for replace_after_start in [false, true] {
        let mut app = app();
        source(&mut app, "sound.wav", Duration::from_secs(1));
        let owner = chart(
            &mut app,
            &format!(
                r#"<state id="active">{}<transition event="error.activity.sound" target="failed"/></state><final id="failed"/>"#,
                activity("sound.wav", "")
            ),
        );
        if replace_after_start {
            app.update();
        }
        let handle = app.world().resource::<Sources>().handles["sound.wav"].clone();
        app.world_mut()
            .resource_mut::<Assets<AudioSource>>()
            .get_mut(&handle)
            .unwrap()
            .bytes = vec![0, 1, 2].into();
        app.update();
        let failures: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<AudioDiagnostic>>()
            .drain()
            .collect();
        assert_eq!(failures.len(), 1);
        assert!(failures[0].reason.contains("cannot decode audio"));
        assert!(paths(&app).is_empty());
        app.update();
        assert_eq!(
            app.world()
                .get::<StateTreeInstance>(owner)
                .unwrap()
                .status(),
            &InstanceStatus::Complete
        );
    }
}

#[test]
fn unavailable_and_disappearing_sinks_fail_the_activity() {
    for disappear in [false, true] {
        let mut app = app();
        source(&mut app, "sound.wav", Duration::from_secs(1));
        let owner = chart(
            &mut app,
            &format!(
                r#"<state id="active">{}<transition event="error.activity.sound" target="failed"/></state><final id="failed"/>"#,
                activity("sound.wav", "")
            ),
        );
        if disappear {
            app.update();
            app.update();
            let player = app.world().resource::<Runtime>().voices[0]
                .entity()
                .unwrap();
            app.world_mut()
                .entity_mut(player)
                .remove::<(AudioSink, OfflineOutput)>();
        } else {
            app.world_mut().run_schedule(Update);
            app.world_mut()
                .resource_mut::<Time<Real>>()
                .advance_by(Duration::from_secs(3));
        }
        app.world_mut().run_schedule(Update);
        let failures: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<AudioDiagnostic>>()
            .drain()
            .collect();
        assert_eq!(failures.len(), 1);
        assert!(failures[0].reason.contains(if disappear {
            "audio sink disappeared"
        } else {
            "device unavailable"
        }));
        assert!(paths(&app).is_empty());
        app.world_mut().run_schedule(Update);
        assert_eq!(
            app.world()
                .get::<StateTreeInstance>(owner)
                .unwrap()
                .status(),
            &InstanceStatus::Complete
        );
    }
}

#[test]
fn authored_speed_controls_real_sink_completion() {
    let mut app = app();
    source(&mut app, "sound.wav", Duration::from_secs(1));
    let owner = chart(
        &mut app,
        &format!(
            r#"<state id="active">{}<transition event="done.activity.sound" target="done"/></state><final id="done"/>"#,
            activity("sound.wav", r#"<ist:arg name="speed" expr="2"/>"#)
        ),
    );
    app.update();
    let player = app.world().resource::<Runtime>().voices[0]
        .entity()
        .unwrap();
    assert_eq!(app.world().get::<AudioSink>(player).unwrap().speed(), 2.0);
    advance_time(&mut app, Duration::from_millis(400));
    assert_eq!(paths(&app), ["sound.wav"]);
    advance_time(&mut app, Duration::from_millis(200));
    assert!(paths(&app).is_empty());
    app.update();
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .status(),
        &InstanceStatus::Complete
    );
}
