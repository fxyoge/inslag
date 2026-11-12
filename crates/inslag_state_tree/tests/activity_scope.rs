use bevy_app::App;
use bevy_asset::{AssetPlugin, Assets};
use bevy_ecs::{
    prelude::*,
    schedule::{ScheduleLabel, SingleThreadedExecutor},
    system::RunSystemOnce,
};
use bevy_time::TimePlugin;
use inslag_state_tree::prelude::*;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, ScheduleLabel)]
struct Tick;

#[derive(StateTreeInput)]
struct Work {
    #[state_tree(activity_id)]
    id: String,
}
impl StateTreeActivity for Work {
    type Output = ();
    type Error = ();
}

#[derive(EntityEvent, StateTreeEvent)]
#[state_tree(name = "capture")]
struct Capture {
    entity: Entity,
}
#[derive(EntityEvent, StateTreeEvent)]
#[state_tree(name = "restart")]
struct Restart {
    entity: Entity,
}
#[derive(EntityEvent, StateTreeEvent)]
#[state_tree(name = "notified")]
struct Notified {
    entity: Entity,
}
#[derive(Message, StateTreeInput)]
struct Captured(#[state_tree(activity = "activity")] Option<StateTreeActivityToken<Work>>);

fn configure(config: &mut App) {
    config
        .add_state_tree_event::<Capture>()
        .add_state_tree_event::<Restart>()
        .add_state_tree_event::<Notified>()
        .add_state_tree_activity::<Work>("work")
        .add_state_tree_effect::<Captured>("capture");
}

#[test]
fn typed_activity_tokens_are_scoped_to_runtime_and_generation() {
    let mut app = App::new();
    app.add_plugins((TimePlugin, AssetPlugin::default()))
        .init_schedule(Tick)
        .add_plugins(StateTreePlugin::new(Tick));
    configure(&mut app);
    app.edit_schedule(Tick, |schedule| {
        schedule.set_executor(SingleThreadedExecutor::new());
    });
    let source = r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:ist="urn:inslag-state-tree" datamodel="luau" initial="active">
      <datamodel><data id="notifications" expr="0"/></datamodel>
      <state id="active"><ist:activity id="task" name="work"/>
        <transition event="capture"><ist:effect name="capture"><ist:arg name="activity" expr="'task'"/></ist:effect></transition>
        <transition event="restart" target="active"/>
        <transition event="notified"><assign location="notifications" expr="notifications + 1"/></transition>
      </state></scxml>"#;
    let definition = inslag_state_tree::scxml::compile(
        &inslag_state_tree::scxml::parse("scope.scxml", source).unwrap(),
    )
    .unwrap();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition);
    let owner = app
        .world_mut()
        .spawn(StateTreeInstance::new(handle.clone()))
        .id();
    app.world_mut().run_schedule(Tick);
    let starts = app
        .world()
        .resource::<Messages<StateTreeActivityLifecycle<Work>>>();
    assert!(starts.get_cursor().read(starts).any(|message| matches!(
        message,
        StateTreeActivityLifecycle::Started { activity, .. } if activity.id == "task"
    )));
    let capture = |app: &mut App| {
        app.world_mut()
            .run_system_once(move |mut events: EntityStateTreeEventWriter| {
                events.write(Capture { entity: owner });
            })
            .unwrap();
        app.world_mut().run_schedule(Tick);
        app.world_mut()
            .resource_mut::<Messages<Captured>>()
            .drain()
            .next()
            .unwrap()
            .0
            .unwrap()
    };
    let first = capture(&mut app);
    assert_eq!(first.entity(), owner);
    assert!(
        app.world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .is_activity_active(first)
    );
    app.world_mut()
        .run_system_once(move |mut events: EntityStateTreeEventWriter| {
            events.write(Restart { entity: owner });
            assert!(events.write_for_activity(first, Notified { entity: owner }));
        })
        .unwrap();
    app.world_mut().run_schedule(Tick);
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .data("notifications"),
        Some(&LuauValue::Integer(0))
    );
    let second = capture(&mut app);
    assert_ne!(first, second);
    assert!(
        !app.world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .is_activity_active(first)
    );
    app.world_mut()
        .run_system_once(move |mut events: EntityStateTreeEventWriter| {
            assert!(!events.write_for_activity(first, Notified { entity: owner }));
            assert!(events.write_for_activity(second, Notified { entity: owner }));
        })
        .unwrap();
    app.world_mut().run_schedule(Tick);
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .data("notifications"),
        Some(&LuauValue::Integer(1))
    );
    app.world_mut()
        .entity_mut(owner)
        .insert(StateTreeInstance::new(handle));
    app.world_mut().run_schedule(Tick);
    let third = capture(&mut app);
    assert_ne!(second, third);
    assert!(
        !app.world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .is_activity_active(second)
    );
}
