use std::time::Duration;

use bevy_app::{App, Update};
use bevy_asset::{AssetPlugin, Assets};
use bevy_ecs::{entity_disabling::Disabled, prelude::*, system::RunSystemOnce};
use bevy_time::{Time, TimePlugin, TimeUpdateStrategy};
use inslag_state_tree::prelude::*;

#[derive(EntityEvent, StateTreeEvent)]
#[state_tree(name = "early")]
struct Early {
    entity: Entity,
}

#[derive(Event, StateTreeEvent)]
#[state_tree(name = "late", broadcast)]
struct Late;

#[derive(Message, inslag_state_tree::StateTreeInput)]
struct Published(#[state_tree(owner)] Entity);

#[derive(inslag_state_tree::StateTreeInput)]
struct Work;

impl StateTreeActivity for Work {
    type Output = ();
    type Error = ();
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TimePlugin,
        AssetPlugin::default(),
        StateTreePlugin::new(Update),
    ));
    app
}

fn spawn(world: &mut World, source: &str) -> Entity {
    let definition = inslag_state_tree::scxml::compile(
        &inslag_state_tree::scxml::parse("runtime.scxml", source).unwrap(),
    )
    .unwrap();
    let handle = world
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition);
    world.spawn(StateTreeInstance::new(handle)).id()
}

fn counter(world: &World, entity: Entity, name: &str) -> i64 {
    match world.get::<StateTreeInstance>(entity).unwrap().data(name) {
        Some(LuauValue::Integer(value)) => *value,
        value => panic!("unexpected counter: {value:?}"),
    }
}

fn broadcast(mut events: BroadcastStateTreeEventWriter) {
    events.write(Late);
}

const RUNNING: &str = r#"
    <scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:ist="urn:inslag-state-tree"
           datamodel="luau" initial="active">
      <datamodel><data id="count" expr="0"/><data id="entries" expr="0"/></datamodel>
      <state id="active">
        <onentry><assign location="entries" expr="entries + 1"/></onentry>
        <ist:activity id="job" name="work"/>
        <transition event="early"><assign location="count" expr="count + 1"/></transition>
        <transition event="late"><assign location="count" expr="count + 10"/></transition>
        <transition ist:after="10" target="done"/>
      </state>
      <state id="unused"><onentry><raise event="late"/></onentry></state>
      <final id="done"/>
    </scxml>
"#;

#[test]
fn additions_can_precede_plugin_installation() {
    let mut app = App::new();
    app.add_state_tree_event::<Late>()
        .add_state_tree_effect::<Published>("publish")
        .add_state_tree_activity(inslag_state_tree::Activity::<Work>::new("work").with_lifecycle())
        .add_plugins((
            TimePlugin,
            AssetPlugin::default(),
            StateTreePlugin::new(Update),
        ));
    let inventory = app.world().resource::<StateTreeProtocolInventory>();
    assert_eq!(inventory.events().collect::<Vec<_>>(), ["late"]);
    assert_eq!(inventory.effects().collect::<Vec<_>>(), ["publish"]);
    assert_eq!(
        inventory.activities().collect::<Vec<_>>(),
        ["ist.spawn", "work"]
    );
    app.update();
}

#[test]
fn missing_registrations_arrive_incrementally_and_typed_messages_are_maintained() {
    let mut app = app();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        20,
    )));
    let entity = spawn(
        app.world_mut(),
        r#"
        <scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:ist="urn:inslag-state-tree" initial="active">
          <state id="active">
            <onentry><ist:effect name="publish"/></onentry>
            <ist:activity id="job" name="work"/>
            <transition event="late"/>
          </state>
        </scxml>
    "#,
    );
    app.update();
    assert_eq!(
        app.world()
            .get::<StateTreeLinkFailure>(entity)
            .unwrap()
            .error()
            .issues()
            .len(),
        3
    );

    app.world_mut().add_state_tree_event::<Late>();
    app.update();
    let failure = app
        .world()
        .get::<StateTreeLinkFailure>(entity)
        .unwrap()
        .clone();
    assert_eq!(failure.error().issues().len(), 2);
    app.add_state_tree_event::<Late>();
    app.update();
    assert!(std::ptr::eq(
        failure.error(),
        app.world()
            .get::<StateTreeLinkFailure>(entity)
            .unwrap()
            .error()
    ));

    app.world_mut()
        .add_state_tree_effect::<Published>("publish");
    app.update();
    assert_eq!(
        app.world()
            .get::<StateTreeLinkFailure>(entity)
            .unwrap()
            .error()
            .issues()
            .len(),
        1
    );

    app.world_mut()
        .add_state_tree_activity(inslag_state_tree::Activity::<Work>::new("work").with_lifecycle());
    app.update();
    assert!(app.world().get::<StateTreeLinkFailure>(entity).is_none());
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .status(),
        &InstanceStatus::Running
    );
    let messages = app.world().resource::<Messages<Published>>();
    assert_eq!(
        messages.get_cursor().read(messages).next().unwrap().0,
        entity
    );
    assert_eq!(
        app.world()
            .resource::<Messages<StateTreeActivityLifecycle<Work>>>()
            .len(),
        1
    );
    app.update();
    app.update();
    assert!(app.world().resource::<Messages<Published>>().is_empty());
    assert!(
        app.world()
            .resource::<Messages<StateTreeActivityLifecycle<Work>>>()
            .is_empty()
    );
}

#[test]
fn event_addition_preserves_queued_events_data_timers_and_activity_tokens() {
    let mut app = app();
    app.add_state_tree_event::<Early>()
        .add_state_tree_activity(inslag_state_tree::Activity::<Work>::new("work").with_lifecycle());
    let first = spawn(app.world_mut(), RUNNING);
    let handle = app
        .world()
        .get::<StateTreeInstance>(first)
        .unwrap()
        .definition()
        .clone();
    let disabled = app
        .world_mut()
        .spawn(StateTreeInstance::new(handle.clone()))
        .id();
    app.world_mut().run_schedule(Update);
    let token = app
        .world_mut()
        .resource_mut::<Messages<StateTreeActivityLifecycle<Work>>>()
        .drain()
        .find_map(|message| match message {
            StateTreeActivityLifecycle::Started { token, .. } if token.entity() == first => {
                Some(token)
            }
            _ => None,
        })
        .unwrap();
    app.world_mut().entity_mut(disabled).insert(Disabled);
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(4));
    app.world_mut().run_schedule(Update);
    app.world_mut()
        .run_system_once(move |mut events: EntityStateTreeEventWriter| {
            assert!(events.write(Early { entity: first }));
        })
        .unwrap();

    app.world_mut().add_state_tree_event::<Late>();
    let fresh = app.world_mut().spawn(StateTreeInstance::new(handle)).id();
    app.world_mut().run_system_once(broadcast).unwrap();
    app.world_mut().run_schedule(Update);
    assert_eq!(counter(app.world(), first, "count"), 11);
    assert_eq!(counter(app.world(), first, "entries"), 1);
    assert_eq!(counter(app.world(), fresh, "count"), 10);
    let messages = app
        .world_mut()
        .resource_mut::<Messages<StateTreeActivityLifecycle<Work>>>()
        .drain()
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 1);
    assert!(
        matches!(messages[0], StateTreeActivityLifecycle::Started { token, .. } if token.entity() == fresh)
    );

    app.world_mut().entity_mut(disabled).remove::<Disabled>();
    app.world_mut().run_system_once(broadcast).unwrap();
    app.world_mut().run_schedule(Update);
    assert_eq!(counter(app.world(), disabled, "count"), 10);
    assert_eq!(counter(app.world(), disabled, "entries"), 1);
    assert!(
        app.world_mut()
            .run_system_once(move |mut outcomes: StateTreeActivityOutcomeWriter<Work>| {
                outcomes.complete(token, ())
            })
            .unwrap()
    );

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(6));
    app.world_mut().run_schedule(Update);
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(first)
            .unwrap()
            .status(),
        &InstanceStatus::Complete
    );
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(fresh)
            .unwrap()
            .status(),
        &InstanceStatus::Running
    );
}

#[test]
fn commands_can_add_and_deliver_an_event_in_the_same_schedule_run() {
    let mut app = app();
    app.add_state_tree_event::<Early>()
        .add_state_tree_activity(inslag_state_tree::Activity::<Work>::new("work").with_lifecycle());
    let entity = spawn(app.world_mut(), RUNNING);
    app.update();
    app.add_systems(
        Update,
        (
            |mut commands: Commands| {
                commands.queue(|world: &mut World| {
                    world.add_state_tree_event::<Late>();
                });
            },
            broadcast,
        )
            .chain()
            .in_set(StateTreeSet::QueueEvents),
    );
    app.update();
    assert_eq!(counter(app.world(), entity, "count"), 10);
    assert_eq!(counter(app.world(), entity, "entries"), 1);
}
