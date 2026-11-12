#![cfg(feature = "persistence")]
use bevy_app::{App, TaskPoolPlugin, Update};
use bevy_asset::{AssetPlugin, Assets, Handle};
use bevy_ecs::{prelude::*, system::RunSystemOnce};
use bevy_time::{Time, TimePlugin};
use inslag_state_tree::{persistence::Checkpoint, prelude::*};
use std::time::Duration;

fn lifecycle(
    mut messages: MessageReader<StateTreeActivityLifecycle<Work>>,
    mut runs: ResMut<Runs>,
    mut starts: ResMut<Starts>,
) {
    for message in messages.read() {
        match message {
            StateTreeActivityLifecycle::Started { token, activity } => {
                starts.0 += 1;
                runs.0.push((*token, activity.0))
            }
            StateTreeActivityLifecycle::Cancelled { token } => {
                runs.0.retain(|(current, _)| current != token)
            }
        }
    }
}
#[derive(inslag_state_tree::StateTreeInput)]
struct Work(#[state_tree(rename = "setting")] i64);
impl StateTreeActivity for Work {
    type Output = ();
    type Error = ();
}
type Token = StateTreeActivityToken<Work>;
#[derive(EntityEvent, StateTreeEvent)]
#[state_tree(name = "advance")]
struct Advance {
    entity: Entity,
}
#[derive(Resource, Default)]
struct Runs(Vec<(Token, i64)>);
#[derive(Resource, Default)]
struct Starts(usize);
#[derive(Resource)]
struct ConsumersEnabled(bool);
fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        AssetPlugin::default(),
        StateTreePlugin::new(Update),
    ))
    .init_resource::<Runs>()
    .init_resource::<Starts>()
    .insert_resource(ConsumersEnabled(true))
    .add_systems(
        Update,
        lifecycle
            .in_set(StateTreeSet::HandleActivities)
            .run_if(|enabled: Res<ConsumersEnabled>| enabled.0),
    );
    configure(&mut app);
    app
}
fn definition(updated: bool) -> StateTreeDefinition {
    let data = if updated {
        r#"<data id="counter" expr="999"/><data id="optional" expr="123"/><data id="introduced" expr="counter + 10"/>"#
    } else {
        r#"<data id="counter" expr="2"/><data id="optional" expr="nil"/><data id="removed" expr="5"/>"#
    };
    let state = if updated { "replacement" } else { "mounted" };
    xml(&format!(
        r#"
      <datamodel>{data}<data id="entries" expr="0"/></datamodel>
      <state id="{state}">
        <onentry><assign location="entries" expr="entries + 1"/></onentry>
        <ist:activity id="job" name="work"><ist:arg name="setting" expr="counter + config.extra"/></ist:activity>
        <transition event="advance" target="other"><assign location="counter" expr="counter + 1"/></transition>
        <transition ist:after="10s" target="done"/>
      </state>
      <state id="other"><transition event="advance" target="{state}"/></state>
      <final id="done"/>
    "#
    ))
}
fn xml(body: &str) -> StateTreeDefinition {
    let source = format!(
        r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:ist="urn:inslag-state-tree" datamodel="luau">{body}</scxml>"#
    );
    inslag_state_tree::scxml::compile(
        &inslag_state_tree::scxml::parse("test.scxml", &source).unwrap(),
    )
    .unwrap()
}
fn spawn(app: &mut App, definition: StateTreeDefinition) -> (Entity, Handle<StateTreeDefinition>) {
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition);
    let entity = app
        .world_mut()
        .spawn((
            StateTreeInstance::new(handle.clone()),
            PersistStateTree,
            configuration(2),
        ))
        .id();
    app.world_mut().run_schedule(Update);
    (entity, handle)
}
fn advance(app: &mut App, entity: Entity) {
    app.world_mut()
        .run_system_once(move |mut writer: EntityStateTreeEventWriter| {
            writer.write(Advance { entity })
        })
        .unwrap();
    app.world_mut().run_schedule(Update);
}
fn data(app: &App, entity: Entity, name: &str) -> Option<LuauValue> {
    app.world()
        .get::<StateTreeInstance>(entity)
        .unwrap()
        .data(name)
        .cloned()
}
fn configure(config: &mut App) {
    config
        .add_state_tree_event::<Advance>()
        .add_state_tree_activity::<Work>("work");
}
fn configuration(extra: i64) -> StateTreeConfiguration {
    StateTreeConfiguration::new([("extra".into(), LuauValue::Integer(extra))])
}

#[test]
fn checkpoint_only_restores_datamodel_then_enters_current_initial_configuration() {
    let mut source = app();
    let (owner, _) = spawn(&mut source, definition(false));
    advance(&mut source, owner);
    assert_eq!(data(&source, owner, "counter"), Some(LuauValue::Integer(3)));
    let checkpoint = Checkpoint::capture(source.world_mut()).unwrap();
    let bytes = serde_json::to_vec(&checkpoint).unwrap();
    let checkpoint: Checkpoint = serde_json::from_slice(&bytes).unwrap();
    let mut target = app();
    target
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition(true));
    checkpoint.restore(target.world_mut()).unwrap();
    let entity = target
        .world_mut()
        .query_filtered::<Entity, With<StateTreeInstance>>()
        .single(target.world())
        .unwrap();
    target
        .world_mut()
        .entity_mut(entity)
        .insert(configuration(40));
    target.world_mut().run_schedule(Update);
    assert_eq!(
        data(&target, entity, "counter"),
        Some(LuauValue::Integer(3))
    );
    assert_eq!(data(&target, entity, "optional"), Some(LuauValue::Nil));
    assert_eq!(
        data(&target, entity, "introduced"),
        Some(LuauValue::Integer(13))
    );
    assert_eq!(data(&target, entity, "removed"), None);
    assert_eq!(
        data(&target, entity, "entries"),
        Some(LuauValue::Integer(2))
    );
    assert_eq!(data(&target, entity, "config"), None);
    assert_eq!(target.world().resource::<Runs>().0[0].1, 43);
    assert_eq!(
        target
            .world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .active_state_names()
            .last(),
        Some("replacement")
    );
}

#[test]
fn hot_reload_restarts_timers_and_activities_and_drops_old_outcomes_and_events() {
    let mut app = app();
    let (entity, handle) = spawn(&mut app, definition(false));
    let old = app.world().resource::<Runs>().0[0].0;
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(7));
    app.world_mut().run_schedule(Update);
    app.world_mut()
        .run_system_once(move |mut writer: StateTreeActivityOutcomeWriter<Work>| {
            assert!(writer.complete(old, ()))
        })
        .unwrap();
    app.world_mut()
        .run_system_once(move |mut writer: EntityStateTreeEventWriter| {
            writer.write(Advance { entity })
        })
        .unwrap();
    app.world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .insert(handle.id(), definition(true))
        .unwrap();
    app.update();
    app.update();
    assert_eq!(data(&app, entity, "counter"), Some(LuauValue::Integer(2)));
    assert_eq!(data(&app, entity, "entries"), Some(LuauValue::Integer(2)));
    assert_eq!(app.world().resource::<Runs>().0.len(), 1);
    assert_ne!(app.world().resource::<Runs>().0[0].0, old);
    assert!(
        !app.world_mut()
            .run_system_once(
                move |mut writer: StateTreeActivityOutcomeWriter<Work>| writer.complete(old, ())
            )
            .unwrap()
    );
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(4));
    app.world_mut().run_schedule(Update);
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .status(),
        &InstanceStatus::Running
    );
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(6));
    app.world_mut().run_schedule(Update);
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .status(),
        &InstanceStatus::Complete
    );
}

#[test]
fn configuration_change_reconstructs_without_overwriting_persistent_data() {
    let mut app = app();
    let (entity, _) = spawn(&mut app, definition(false));
    let old = app.world().resource::<Runs>().0[0].0;
    app.world_mut().entity_mut(entity).insert(configuration(50));
    app.world_mut().run_schedule(Update);
    app.world_mut().run_schedule(Update);
    assert_eq!(app.world().resource::<Runs>().0.len(), 1);
    assert_eq!(app.world().resource::<Runs>().0[0].1, 52);
    assert_ne!(app.world().resource::<Runs>().0[0].0, old);
    assert_eq!(data(&app, entity, "entries"), Some(LuauValue::Integer(2)));
}

#[test]
fn sqlite_restore_restarts_completed_execution_and_replays_current_entry_actions() {
    use inslag_persist::{SavePlugin, WorldSaveExt};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("save.sqlite");
    let mut first = app();
    first.add_plugins(SavePlugin::sqlite(&path));
    let (entity, _) = spawn(&mut first, definition(false));
    first
        .world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(10));
    first.world_mut().run_schedule(Update);
    assert_eq!(
        first
            .world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .status(),
        &InstanceStatus::Complete
    );
    first.world_mut().save().unwrap();
    let mut second = app();
    second.add_plugins(SavePlugin::sqlite(&path));
    second
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition(true));
    second.world_mut().load().unwrap();
    let entity = second
        .world_mut()
        .query_filtered::<Entity, With<StateTreeInstance>>()
        .single(second.world())
        .unwrap();
    second
        .world_mut()
        .entity_mut(entity)
        .insert(configuration(8));
    second.world_mut().run_schedule(Update);
    assert_eq!(
        data(&second, entity, "entries"),
        Some(LuauValue::Integer(2))
    );
    assert_eq!(second.world().resource::<Runs>().0[0].1, 10);
    assert_eq!(
        second
            .world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .status(),
        &InstanceStatus::Running
    );
}

#[test]
fn boot_routing_is_ordinary_scxml_using_persistent_data() {
    let mut app = app();
    let definition = xml(r#"<datamodel><data id="phase" expr="'idle'"/></datamodel>
      <state id="boot"><transition cond="phase == 'working'" target="working"/><transition target="idle"/></state>
      <state id="idle"><transition event="advance" target="working"><assign location="phase" expr="'working'"/></transition></state>
      <state id="working"/>"#);
    let (entity, _) = spawn(&mut app, definition);
    advance(&mut app, entity);
    let checkpoint = Checkpoint::capture(app.world_mut()).unwrap();
    checkpoint.restore(app.world_mut()).unwrap();
    app.world_mut().run_schedule(Update);
    let entity = app
        .world_mut()
        .query_filtered::<Entity, With<StateTreeInstance>>()
        .single(app.world())
        .unwrap();
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .active_state_names()
            .last(),
        Some("working")
    );
}

#[test]
fn configuration_cannot_be_declared_or_assigned_as_datamodel() {
    let document = inslag_state_tree::scxml::parse("invalid", r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" datamodel="luau"><datamodel><data id="config" expr="{}"/></datamodel><state id="active"/></scxml>"#).unwrap();
    assert!(inslag_state_tree::scxml::compile(&document).is_err());
    let document = inslag_state_tree::scxml::parse("invalid", r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" datamodel="luau"><state id="active"><onentry><assign location="config.extra" expr="1"/></onentry></state></scxml>"#).unwrap();
    assert!(inslag_state_tree::scxml::compile(&document).is_err());
}

#[test]
fn configuration_is_recursively_read_only_inside_expressions() {
    let mut app = app();
    let definition = xml(r#"<datamodel><data id="counter" expr="0"/></datamodel>
      <state id="active"><onentry><assign location="counter" expr="(function() config.nested.value = 3 return 9 end)()"/></onentry></state>"#);
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition);
    let entity = app
        .world_mut()
        .spawn((
            StateTreeInstance::new(handle),
            StateTreeConfiguration::new([(
                "nested".into(),
                LuauValue::Record([("value".into(), LuauValue::Integer(1))].into()),
            )]),
        ))
        .id();
    app.world_mut().run_schedule(Update);
    assert!(matches!(
        app.world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .status(),
        InstanceStatus::Faulted(_)
    ));
    assert_eq!(data(&app, entity, "counter"), Some(LuauValue::Integer(0)));
}

#[test]
fn configuration_and_triggering_event_are_available_together() {
    let mut app = app();
    let definition = xml(r#"<datamodel><data id="counter" expr="0"/></datamodel>
      <state id="active"><transition event="advance"><assign location="counter" expr="if _event.name == 'advance' then config.extra else -1"/></transition></state>"#);
    let (entity, _) = spawn(&mut app, definition);
    advance(&mut app, entity);
    assert_eq!(data(&app, entity, "counter"), Some(LuauValue::Integer(2)));
}

#[test]
fn disabled_instances_discard_old_execution_before_they_are_enabled_again() {
    use bevy_ecs::entity_disabling::Disabled;
    let mut app = app();
    let (entity, handle) = spawn(&mut app, definition(false));
    app.world_mut().entity_mut(entity).insert(Disabled);
    app.world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .insert(handle.id(), definition(true))
        .unwrap();
    app.world_mut().run_schedule(Update);
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .status(),
        &InstanceStatus::Pending
    );
    app.world_mut().entity_mut(entity).remove::<Disabled>();
    app.world_mut().run_schedule(Update);
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .active_state_names()
            .last(),
        Some("replacement")
    );
    assert_eq!(data(&app, entity, "entries"), Some(LuauValue::Integer(2)));
}

#[test]
fn restore_discards_unconsumed_activity_starts_from_the_outgoing_execution() {
    let mut app = app();
    app.world_mut().resource_mut::<ConsumersEnabled>().0 = false;
    spawn(&mut app, definition(false));
    assert_eq!(app.world().resource::<Starts>().0, 0);
    let checkpoint = Checkpoint::capture(app.world_mut()).unwrap();
    checkpoint.restore(app.world_mut()).unwrap();
    let entity = app
        .world_mut()
        .query_filtered::<Entity, With<StateTreeInstance>>()
        .single(app.world())
        .unwrap();
    app.world_mut().entity_mut(entity).insert(configuration(4));
    app.world_mut().resource_mut::<ConsumersEnabled>().0 = true;
    app.world_mut().run_schedule(Update);
    assert_eq!(app.world().resource::<Starts>().0, 1);
    assert_eq!(app.world().resource::<Runs>().0.len(), 1);
}

#[test]
fn reinserting_a_detached_instance_reconstructs_against_current_content() {
    let mut app = app();
    let (entity, handle) = spawn(&mut app, definition(false));
    let detached = app
        .world_mut()
        .entity_mut(entity)
        .take::<StateTreeInstance>()
        .unwrap();
    app.world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .insert(handle.id(), definition(true))
        .unwrap();
    app.update();
    app.update();
    app.world_mut().entity_mut(entity).insert(detached);
    app.world_mut().run_schedule(Update);
    app.world_mut().run_schedule(Update);
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .active_state_names()
            .last(),
        Some("replacement")
    );
    assert_eq!(data(&app, entity, "entries"), Some(LuauValue::Integer(2)));
}

#[test]
fn persistence_is_opt_in_and_restored_instances_keep_their_selection() {
    let mut source = app();
    let (saved, handle) = spawn(&mut source, definition(false));
    let runtime = source
        .world_mut()
        .spawn((StateTreeInstance::new(handle), configuration(2)))
        .id();
    source.world_mut().run_schedule(Update);
    advance(&mut source, runtime);
    let checkpoint = Checkpoint::capture(source.world_mut()).unwrap();
    let mut target = app();
    target
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition(false));
    checkpoint.restore(target.world_mut()).unwrap();
    let restored = target
        .world_mut()
        .query_filtered::<Entity, With<StateTreeInstance>>()
        .single(target.world())
        .unwrap();
    assert!(target.world().get::<PersistStateTree>(restored).is_some());
    assert_eq!(
        source
            .world()
            .get::<StateTreeInstance>(saved)
            .unwrap()
            .data("counter"),
        Some(&LuauValue::Integer(2))
    );
    target
        .world_mut()
        .entity_mut(restored)
        .insert(configuration(2));
    target.world_mut().run_schedule(Update);
    assert_eq!(
        data(&target, restored, "counter"),
        Some(LuauValue::Integer(2))
    );
    let repeated = Checkpoint::capture(target.world_mut()).unwrap();
    repeated.restore(target.world_mut()).unwrap();
    assert!(
        target
            .world_mut()
            .query_filtered::<Entity, With<PersistStateTree>>()
            .single(target.world())
            .is_ok()
    );
    checkpoint.restore(source.world_mut()).unwrap();
    assert_eq!(
        data(&source, runtime, "counter"),
        Some(LuauValue::Integer(3))
    );
}

#[test]
fn unmarked_instances_preserve_pending_events_outcomes_and_timer_deadlines() {
    let mut app = app();
    let definition = xml(r#"
      <datamodel><data id="counter" expr="0"/><data id="completed" expr="false"/></datamodel>
      <state id="mounted">
        <ist:activity id="job" name="work"><ist:arg name="setting" expr="1"/></ist:activity>
        <transition event="advance"><assign location="counter" expr="counter + 1"/></transition>
        <transition event="done.activity.job"><assign location="completed" expr="true"/></transition>
        <transition ist:after="10s" target="done"/>
      </state><final id="done"/>"#);
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition);
    let owner = app.world_mut().spawn(StateTreeInstance::new(handle)).id();
    app.world_mut().run_schedule(Update);
    let token = app.world().resource::<Runs>().0[0].0;
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(7));
    app.world_mut().run_schedule(Update);
    let checkpoint = Checkpoint::capture(app.world_mut()).unwrap();
    app.world_mut()
        .run_system_once(
            move |mut writer: EntityStateTreeEventWriter,
                  mut outcomes: StateTreeActivityOutcomeWriter<Work>| {
                writer.write(Advance { entity: owner });
                assert!(outcomes.complete(token, ()));
            },
        )
        .unwrap();
    checkpoint.restore(app.world_mut()).unwrap();
    app.world_mut().run_schedule(Update);
    assert_eq!(data(&app, owner, "counter"), Some(LuauValue::Integer(1)));
    assert_eq!(
        data(&app, owner, "completed"),
        Some(LuauValue::Boolean(true))
    );
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(3));
    app.world_mut().run_schedule(Update);
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .status(),
        &InstanceStatus::Complete
    );
}

#[test]
fn unmarked_activity_starts_survive_restoring_a_different_saved_selection() {
    let mut app = app();
    app.world_mut().resource_mut::<ConsumersEnabled>().0 = false;
    let definition = xml(
        r#"<state id="mounted"><ist:activity id="job" name="work"><ist:arg name="setting" expr="123"/></ist:activity></state>"#,
    );
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition);
    let owner = app.world_mut().spawn(StateTreeInstance::new(handle)).id();
    app.world_mut().run_schedule(Update);
    let checkpoint = Checkpoint::capture(app.world_mut()).unwrap();
    checkpoint.restore(app.world_mut()).unwrap();
    app.world_mut().resource_mut::<ConsumersEnabled>().0 = true;
    app.world_mut().run_schedule(Update);
    assert_eq!(app.world().resource::<Starts>().0, 1);
    let run = app.world().resource::<Runs>().0[0];
    assert_eq!(run.0.entity(), owner);
    assert_eq!(run.1, 123);
}

#[test]
fn unmarked_controllers_do_not_replay_already_consumed_activity_starts() {
    let mut app = app();
    let definition = xml(
        r#"<state id="mounted"><ist:activity id="job" name="work"><ist:arg name="setting" expr="123"/></ist:activity></state>"#,
    );
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition);
    app.world_mut().spawn(StateTreeInstance::new(handle));
    app.world_mut().run_schedule(Update);
    assert_eq!(app.world().resource::<Starts>().0, 1);
    let token = app.world().resource::<Runs>().0[0].0;
    let checkpoint = Checkpoint::capture(app.world_mut()).unwrap();
    checkpoint.restore(app.world_mut()).unwrap();
    app.world_mut().run_schedule(Update);
    assert_eq!(app.world().resource::<Starts>().0, 1);
    assert_eq!(app.world().resource::<Runs>().0.len(), 1);
    assert_eq!(app.world().resource::<Runs>().0[0].0, token);
}
