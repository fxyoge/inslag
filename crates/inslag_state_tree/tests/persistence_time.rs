#![cfg(feature = "persistence")]

use std::time::Duration;

use bevy_app::{App, TaskPoolPlugin, Update};
use bevy_asset::{AssetPlugin, Assets};
use bevy_ecs::{prelude::*, system::RunSystemOnce};
use bevy_time::{Time, TimePlugin};
use inslag_state_tree::{persistence::Checkpoint, prelude::*};

const SAMPLER: &str = r#"
    <datamodel>
        <data id="game" expr="-1"/>
        <data id="session" expr="-1"/>
        <data id="vm" expr="-1"/>
        <data id="configured" expr="-1"/>
    </datamodel>
    <state>
        <transition event="sample">
            <assign location="game" expr="_time"/>
            <assign location="session" expr="_session_time"/>
            <assign location="vm" expr="(function() return _time + _session_time end)()"/>
            <assign location="configured" expr="_time + _session_time + (config.extra or 0)"/>
        </transition>
    </state>
"#;

const DEADLINE: &str = r#"
    <datamodel>
        <data id="deadline" expr="_time + 10"/>
        <data id="finished" expr="-1"/>
        <data id="session_finished" expr="-1"/>
    </datamodel>
    <state id="boot">
        <transition cond="deadline &lt;= _time" target="done"/>
        <transition target="waiting"/>
    </state>
    <state id="waiting">
        <transition ist:after="deadline - _time" target="done"/>
    </state>
    <final id="done">
        <onentry>
            <assign location="finished" expr="_time"/>
            <assign location="session_finished" expr="_session_time"/>
        </onentry>
    </final>
"#;

#[derive(EntityEvent, StateTreeEvent)]
#[state_tree(name = "sample")]
struct Sample {
    entity: Entity,
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        AssetPlugin::default(),
        StateTreePlugin::new(Update),
    ))
    .add_state_tree_event::<Sample>();
    app
}

fn definition(name: &str, body: &str) -> StateTreeDefinition {
    let source = format!(
        r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:ist="urn:inslag-state-tree" datamodel="luau">{body}</scxml>"#,
    );
    inslag_state_tree::scxml::compile(&inslag_state_tree::scxml::parse(name, &source).unwrap())
        .unwrap()
}

fn spawn(app: &mut App, name: &str, body: &str) -> Entity {
    let definition = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition(name, body));
    let entity = app
        .world_mut()
        .spawn((StateTreeInstance::new(definition), PersistStateTree))
        .id();
    app.world_mut().run_schedule(Update);
    entity
}

fn owner(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<PersistStateTree>>()
        .single(app.world())
        .unwrap()
}

fn advance(app: &mut App, duration: Duration) {
    app.world_mut().resource_mut::<Time>().advance_by(duration);
    app.world_mut().run_schedule(Update);
}

fn sample(app: &mut App, entity: Entity) {
    app.world_mut()
        .run_system_once(move |mut events: EntityStateTreeEventWriter| {
            events.write(Sample { entity });
        })
        .unwrap();
    app.world_mut().run_schedule(Update);
}

fn number(app: &App, entity: Entity, name: &str) -> f64 {
    match app
        .world()
        .get::<StateTreeInstance>(entity)
        .unwrap()
        .data(name)
    {
        Some(LuauValue::Number(value)) => *value,
        Some(LuauValue::Integer(value)) => *value as f64,
        other => panic!("expected time, got {other:?}"),
    }
}

fn clock(app: &App) -> (Duration, Duration) {
    let time = app.world().resource::<Time>();
    let game = app.world().resource::<GameTime>();
    (game.elapsed(time), game.session_elapsed(time))
}

fn checkpoint(app: &mut App) -> Checkpoint {
    let checkpoint = Checkpoint::capture(app.world_mut()).unwrap();
    serde_json::from_slice(&serde_json::to_vec(&checkpoint).unwrap()).unwrap()
}

#[test]
fn capture_samples_current_time_without_running_or_changing_the_tree() {
    let mut app = app();
    let entity = spawn(&mut app, "clock", SAMPLER);
    let tree_changed = app
        .world()
        .entity(entity)
        .get_ref::<StateTreeInstance>()
        .unwrap()
        .last_changed();
    let clock_changed = app
        .world()
        .get_resource_ref::<GameTime>()
        .unwrap()
        .last_changed();
    for _ in 0..100 {
        advance(&mut app, Duration::from_millis(10));
    }
    assert_eq!(number(&app, entity, "game"), -1.);
    assert_eq!(
        app.world()
            .entity(entity)
            .get_ref::<StateTreeInstance>()
            .unwrap()
            .last_changed(),
        tree_changed
    );
    assert_eq!(
        app.world()
            .get_resource_ref::<GameTime>()
            .unwrap()
            .last_changed(),
        clock_changed
    );
    let checkpoint = checkpoint(&mut app);
    assert_eq!(
        clock(&app),
        (Duration::from_secs(1), Duration::from_secs(1))
    );
    checkpoint.restore(app.world_mut()).unwrap();
    let entity = owner(&mut app);
    sample(&mut app, entity);
    assert_eq!(number(&app, entity, "game"), 1.);
    assert_eq!(number(&app, entity, "session"), 0.);
    assert_eq!(number(&app, entity, "vm"), 1.);
    assert_eq!(number(&app, entity, "configured"), 1.);
}

#[test]
fn restoring_into_a_fresh_app_uses_saved_game_time_and_a_new_session() {
    let mut source = app();
    spawn(&mut source, "clock", SAMPLER);
    advance(&mut source, Duration::from_secs(103));
    let checkpoint = checkpoint(&mut source);
    for runtime_elapsed in [Duration::ZERO, Duration::from_secs(400)] {
        let mut target = app();
        advance(&mut target, runtime_elapsed);
        target
            .world_mut()
            .resource_mut::<Assets<StateTreeDefinition>>()
            .add(definition("clock", SAMPLER));
        checkpoint.restore(target.world_mut()).unwrap();
        let entity = owner(&mut target);
        sample(&mut target, entity);
        assert_eq!(number(&target, entity, "game"), 103.);
        assert_eq!(number(&target, entity, "session"), 0.);
        advance(&mut target, Duration::from_millis(125));
        sample(&mut target, entity);
        assert_eq!(number(&target, entity, "game"), 103.125);
        assert_eq!(number(&target, entity, "session"), 0.125);
        assert_eq!(number(&target, entity, "vm"), 103.25);
        assert_eq!(number(&target, entity, "configured"), 103.25);
        assert_eq!(
            target.world().resource::<Time>().elapsed(),
            runtime_elapsed + Duration::from_millis(125)
        );
    }
}

#[test]
fn repeated_restore_rewinds_game_time_without_rewinding_bevy_time() {
    let mut app = app();
    spawn(&mut app, "clock", SAMPLER);
    advance(&mut app, Duration::from_secs(3));
    let checkpoint = checkpoint(&mut app);
    for elapsed in [Duration::from_secs(100), Duration::from_secs(200)] {
        advance(&mut app, elapsed);
        let runtime_elapsed = app.world().resource::<Time>().elapsed();
        checkpoint.restore(app.world_mut()).unwrap();
        assert_eq!(clock(&app), (Duration::from_secs(3), Duration::ZERO));
        assert_eq!(app.world().resource::<Time>().elapsed(), runtime_elapsed);
        let entity = owner(&mut app);
        sample(&mut app, entity);
        assert_eq!(number(&app, entity, "game"), 3.);
        assert_eq!(number(&app, entity, "session"), 0.);
    }
}

#[test]
fn saved_deadline_rearms_only_its_remaining_duration_after_a_century() {
    let century = Duration::from_secs(100 * 365 * 24 * 60 * 60);
    let mut source = app();
    advance(&mut source, century);
    spawn(&mut source, "deadline", DEADLINE);
    advance(&mut source, Duration::from_secs(3));
    let checkpoint = checkpoint(&mut source);
    let mut target = app();
    advance(&mut target, Duration::from_secs(4));
    target
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition("deadline", DEADLINE));
    checkpoint.restore(target.world_mut()).unwrap();
    target.world_mut().run_schedule(Update);
    let entity = owner(&mut target);
    advance(&mut target, Duration::from_millis(6999));
    assert_eq!(
        target
            .world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .status(),
        &InstanceStatus::Running
    );
    advance(&mut target, Duration::from_millis(1));
    assert_eq!(
        target
            .world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .status(),
        &InstanceStatus::Complete
    );
    assert_eq!(
        number(&target, entity, "finished"),
        (century + Duration::from_secs(10)).as_secs_f64()
    );
    assert_eq!(number(&target, entity, "session_finished"), 7.);
}

#[test]
fn overdue_saved_deadline_routes_without_arming_a_zero_duration_timer() {
    let mut app = app();
    spawn(&mut app, "deadline", DEADLINE);
    advance(&mut app, Duration::from_secs(3));
    let checkpoint = checkpoint(&mut app);
    checkpoint.restore(app.world_mut()).unwrap();
    advance(&mut app, Duration::from_secs(8));
    let entity = owner(&mut app);
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .status(),
        &InstanceStatus::Complete
    );
    assert_eq!(number(&app, entity, "finished"), 11.);
    assert_eq!(number(&app, entity, "session_finished"), 8.);
}

#[test]
fn repeated_save_and_load_keeps_integer_nanoseconds_after_a_century() {
    let mut app = app();
    spawn(&mut app, "clock", SAMPLER);
    let initial = Duration::new(100 * 365 * 24 * 60 * 60, 123_456_789);
    advance(&mut app, initial);
    for _ in 0..10 {
        let checkpoint = checkpoint(&mut app);
        checkpoint.restore(app.world_mut()).unwrap();
        app.world_mut().run_schedule(Update);
    }
    assert_eq!(clock(&app), (initial, Duration::ZERO));
    advance(&mut app, Duration::from_nanos(1));
    assert_eq!(
        clock(&app),
        (initial + Duration::from_nanos(1), Duration::from_nanos(1))
    );
}

#[test]
fn clearing_a_saved_world_starts_a_new_game_without_resetting_bevy_time() {
    let mut app = app();
    advance(&mut app, Duration::from_secs(50));
    Checkpoint::clear(app.world_mut());
    assert_eq!(clock(&app), (Duration::ZERO, Duration::ZERO));
    assert_eq!(
        app.world().resource::<Time>().elapsed(),
        Duration::from_secs(50)
    );
    let entity = spawn(&mut app, "clock", SAMPLER);
    advance(&mut app, Duration::from_secs(2));
    sample(&mut app, entity);
    assert_eq!(number(&app, entity, "game"), 2.);
    assert_eq!(number(&app, entity, "session"), 2.);
}

#[test]
fn hot_reload_preserves_game_and_session_time() {
    let mut app = app();
    spawn(&mut app, "clock", SAMPLER);
    advance(&mut app, Duration::from_secs(10));
    let checkpoint = checkpoint(&mut app);
    checkpoint.restore(app.world_mut()).unwrap();
    let entity = owner(&mut app);
    sample(&mut app, entity);
    advance(&mut app, Duration::from_secs(2));
    let handle = app
        .world()
        .get::<StateTreeInstance>(entity)
        .unwrap()
        .definition()
        .clone();
    app.world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .insert(
            handle.id(),
            definition(
                "clock",
                &SAMPLER.replace("<state>", "<state id=\"replacement\">"),
            ),
        )
        .unwrap();
    app.world_mut().run_schedule(Update);
    app.world_mut().run_schedule(Update);
    assert_eq!(
        clock(&app),
        (Duration::from_secs(12), Duration::from_secs(2))
    );
    sample(&mut app, entity);
    assert_eq!(number(&app, entity, "game"), 12.);
    assert_eq!(number(&app, entity, "session"), 2.);
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .active_state_names()
            .last(),
        Some("replacement")
    );
}

#[test]
fn restoring_game_time_does_not_change_an_unmarked_trees_existing_timer() {
    let mut app = app();
    spawn(&mut app, "clock", SAMPLER);
    let entity = spawn(&mut app, "deadline", DEADLINE);
    app.world_mut()
        .entity_mut(entity)
        .remove::<PersistStateTree>();
    advance(&mut app, Duration::from_secs(3));
    let checkpoint = checkpoint(&mut app);
    advance(&mut app, Duration::from_secs(4));
    checkpoint.restore(app.world_mut()).unwrap();
    advance(&mut app, Duration::from_millis(2999));
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .status(),
        &InstanceStatus::Running
    );
    advance(&mut app, Duration::from_millis(1));
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .status(),
        &InstanceStatus::Complete
    );
    assert_eq!(number(&app, entity, "finished"), 6.);
}

#[test]
fn sqlite_load_restores_the_clock_before_the_first_tree_initializes() {
    use inslag_persist::{SavePlugin, WorldSaveExt};

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("clock.sqlite");
    let mut source = app();
    source.add_plugins(SavePlugin::sqlite(&path));
    spawn(&mut source, "deadline", DEADLINE);
    advance(&mut source, Duration::from_secs(3));
    source.world_mut().save().unwrap();

    let mut target = app();
    target.add_plugins(SavePlugin::sqlite(&path));
    target
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition("deadline", DEADLINE));
    advance(&mut target, Duration::from_secs(400));
    target.world_mut().load().unwrap();
    assert_eq!(clock(&target), (Duration::from_secs(3), Duration::ZERO));
    target.world_mut().run_schedule(Update);
    let entity = owner(&mut target);
    advance(&mut target, Duration::from_secs(7));
    assert_eq!(number(&target, entity, "finished"), 10.);
    assert_eq!(number(&target, entity, "session_finished"), 7.);
}

#[test]
fn failed_restore_leaves_the_outgoing_game_and_session_time_unchanged() {
    let mut source = app();
    spawn(&mut source, "missing", SAMPLER);
    advance(&mut source, Duration::from_secs(3));
    let checkpoint = checkpoint(&mut source);

    let mut target = app();
    let entity = spawn(&mut target, "outgoing", SAMPLER);
    advance(&mut target, Duration::from_secs(40));
    assert!(checkpoint.restore(target.world_mut()).is_err());
    assert_eq!(
        clock(&target),
        (Duration::from_secs(40), Duration::from_secs(40))
    );
    sample(&mut target, entity);
    assert_eq!(number(&target, entity, "game"), 40.);
    assert_eq!(number(&target, entity, "session"), 40.);
}
