use bevy_app::{App, Update};
use bevy_asset::{AssetPlugin, Assets};
use bevy_ecs::{message::MessageCursor, prelude::*, system::RunSystemOnce};
use bevy_time::TimePlugin;
use inslag_state_tree::prelude::*;

#[derive(Clone, StateTreeInput)]
struct Work {
    amount: u32,
}

impl StateTreeActivity for Work {
    type Output = ();
    type Error = ();
}

#[derive(StateTreeInput)]
struct Other;

impl StateTreeActivity for Other {
    type Output = ();
    type Error = ();
}

#[derive(Component, Default)]
struct Available;

#[derive(EntityEvent, StateTreeEvent)]
#[state_tree(name = "next")]
struct Next {
    entity: Entity,
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TimePlugin,
        AssetPlugin::default(),
        StateTreePlugin::new(Update),
    ))
    .add_state_tree_event::<Next>();
    app
}

fn spawn(app: &mut App, states: &str) -> Entity {
    let xml = format!(
        r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:ist="urn:inslag-state-tree" datamodel="luau">{states}</scxml>"#
    );
    let definition = inslag_state_tree::scxml::compile(
        &inslag_state_tree::scxml::parse("activities.scxml", &xml).unwrap(),
    )
    .unwrap();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition);
    app.world_mut().spawn(StateTreeInstance::new(handle)).id()
}

fn next(app: &mut App, entity: Entity) {
    app.world_mut()
        .run_system_once(move |mut events: EntityStateTreeEventWriter| {
            events.write(Next { entity });
        })
        .unwrap();
    app.update();
}

const SINGLE: &str = r#"
    <state id="active">
      <ist:activity id="work" name="work"><ist:arg name="amount" expr="7"/></ist:activity>
      <transition event="next" target="inactive"/>
    </state>
    <state id="inactive"><transition event="next" target="active"/></state>
"#;

#[test]
fn collection_and_marker_need_no_lifecycle_queue() {
    let mut app = app();
    app.add_state_tree_activity(
        Activity::<Work>::new("work")
            .with_marker::<Available>()
            .with_collection(),
    );
    assert!(
        !app.world()
            .contains_resource::<Messages<StateTreeActivityLifecycle<Work>>>()
    );
    let owner = spawn(&mut app, SINGLE);
    app.update();
    assert!(app.world().get::<Available>(owner).is_some());
    let runs = app.world().get::<ActivityRuns<Work>>(owner).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs.iter().next().unwrap().activity.amount, 7);
    assert!(
        !app.world()
            .contains_resource::<Messages<StateTreeActivityLifecycle<Work>>>()
    );
    next(&mut app, owner);
    assert!(app.world().get::<Available>(owner).is_none());
    assert!(app.world().get::<ActivityRuns<Work>>(owner).is_none());
}

#[test]
fn same_update_reentry_preserves_only_the_replacement_run() {
    let mut app = app();
    app.add_state_tree_activity(
        Activity::<Work>::new("work")
            .with_collection()
            .with_marker::<Available>()
            .with_marker::<Available>(),
    );
    let owner = spawn(
        &mut app,
        r#"
      <state id="active">
        <ist:activity id="work" name="work"><ist:arg name="amount" expr="7"/></ist:activity>
        <transition event="next" target="active"/>
      </state>
    "#,
    );
    app.update();
    let previous = app
        .world()
        .get::<ActivityRuns<Work>>(owner)
        .unwrap()
        .iter()
        .next()
        .unwrap()
        .token();
    next(&mut app, owner);
    let runs = app.world().get::<ActivityRuns<Work>>(owner).unwrap();
    assert_eq!(runs.len(), 1);
    assert!(runs.get(previous).is_none());
    assert!(app.world().get::<Available>(owner).is_some());
    app.world_mut()
        .entity_mut(owner)
        .remove::<StateTreeInstance>();
    app.world_mut().flush();
    assert!(app.world().get::<Available>(owner).is_none());
}

#[test]
fn same_type_runs_coexist_and_parent_run_survives_child_exit() {
    let mut app = app();
    app.add_state_tree_activity(
        Activity::<Work>::new("work")
            .with_marker::<Available>()
            .with_collection(),
    );
    let owner = spawn(
        &mut app,
        r#"
      <state id="parent" initial="child">
        <ist:activity id="parent_work" name="work"><ist:arg name="amount" expr="1"/></ist:activity>
        <state id="child">
          <ist:activity id="child_work" name="work"><ist:arg name="amount" expr="2"/></ist:activity>
          <transition event="next" target="sibling"/>
        </state>
        <state id="sibling"><transition event="next" target="outside"/></state>
      </state><state id="outside"/>
    "#,
    );
    app.update();
    let runs = app.world().get::<ActivityRuns<Work>>(owner).unwrap();
    assert_eq!(runs.len(), 2);
    let parent = runs
        .iter()
        .find(|run| run.activity.amount == 1)
        .unwrap()
        .token();
    let child = runs
        .iter()
        .find(|run| run.activity.amount == 2)
        .unwrap()
        .token();
    next(&mut app, owner);
    let runs = app.world().get::<ActivityRuns<Work>>(owner).unwrap();
    assert_eq!(runs.len(), 1);
    assert!(runs.get(parent).is_some());
    assert!(runs.get(child).is_none());
    assert!(app.world().get::<Available>(owner).is_some());
    next(&mut app, owner);
    assert!(app.world().get::<ActivityRuns<Work>>(owner).is_none());
    assert!(app.world().get::<Available>(owner).is_none());
}

#[test]
fn markers_are_shared_across_registration_names_and_activity_types() {
    let mut app = app();
    app.add_state_tree_activity(Activity::<Other>::new("other").with_marker::<Available>())
        .add_state_tree_activity(Activity::<Work>::new("work").with_marker::<Available>())
        .add_state_tree_activity(Activity::<Work>::new("alias").with_marker::<Available>());
    let owner = spawn(
        &mut app,
        r#"
      <state id="parent" initial="middle">
        <ist:activity id="outer" name="other"/>
        <state id="middle" initial="inner">
          <ist:activity id="middle_work" name="work"><ist:arg name="amount" expr="1"/></ist:activity>
          <state id="inner">
            <ist:activity id="inner_work" name="alias"><ist:arg name="amount" expr="2"/></ist:activity>
            <transition event="next" target="middle_done"/>
          </state>
          <state id="middle_done"><transition event="next" target="outer_done"/></state>
        </state>
        <state id="outer_done"><transition event="next" target="outside"/></state>
      </state><state id="outside"/>
    "#,
    );
    app.update();
    assert!(app.world().get::<Available>(owner).is_some());
    next(&mut app, owner);
    assert!(app.world().get::<Available>(owner).is_some());
    next(&mut app, owner);
    assert!(app.world().get::<Available>(owner).is_some());
    next(&mut app, owner);
    assert!(app.world().get::<Available>(owner).is_none());
}

#[test]
fn retained_collections_keep_storage_without_reusing_run_identity() {
    let mut app = app();
    app.add_state_tree_activity(
        Activity::<Work>::new("work")
            .with_collection_retention(ActivityCollectionRetention::KeepEmpty),
    );
    let owner = spawn(&mut app, SINGLE);
    app.update();
    let previous = app
        .world()
        .get::<ActivityRuns<Work>>(owner)
        .unwrap()
        .iter()
        .next()
        .unwrap()
        .token();
    next(&mut app, owner);
    assert!(
        app.world()
            .get::<ActivityRuns<Work>>(owner)
            .unwrap()
            .is_empty()
    );
    next(&mut app, owner);
    let runs = app.world().get::<ActivityRuns<Work>>(owner).unwrap();
    assert_eq!(runs.len(), 1);
    assert_ne!(runs.iter().next().unwrap().token(), previous);
    assert!(runs.get(previous).is_none());
}

#[test]
fn lifecycle_payloads_are_owned_and_independent_of_mutable_runs() {
    let mut app = app();
    app.add_state_tree_activity(
        Activity::<Work>::new("work")
            .with_collection()
            .with_lifecycle(),
    );
    let owner = spawn(&mut app, SINGLE);
    app.update();
    let token = app
        .world()
        .get::<ActivityRuns<Work>>(owner)
        .unwrap()
        .iter()
        .next()
        .unwrap()
        .token();
    app.world_mut()
        .get_mut::<ActivityRuns<Work>>(owner)
        .unwrap()
        .get_mut(token)
        .unwrap()
        .amount = 99;
    next(&mut app, owner);
    let messages = app
        .world()
        .resource::<Messages<StateTreeActivityLifecycle<Work>>>();
    let mut reader = MessageCursor::default();
    let history: Vec<_> = reader.read(messages).collect();
    assert!(matches!(
        history.as_slice(),
        [
            StateTreeActivityLifecycle::Started {
                activity: Work { amount: 7 },
                ..
            },
            StateTreeActivityLifecycle::Cancelled { .. }
        ]
    ));
    assert!(
        !app.world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .is_activity_active(token)
    );
}

#[test]
fn transient_activity_does_not_leave_bindings_after_same_update_exit() {
    let mut app = app();
    app.add_state_tree_activity(
        Activity::<Work>::new("work")
            .with_collection()
            .with_marker::<Available>()
            .with_lifecycle(),
    );
    let owner = spawn(
        &mut app,
        r#"
      <state id="transient">
        <ist:activity id="work" name="work"><ist:arg name="amount" expr="1"/></ist:activity>
        <transition target="outside"/>
      </state><state id="outside"/>
    "#,
    );
    app.update();
    assert!(app.world().get::<Available>(owner).is_none());
    assert!(app.world().get::<ActivityRuns<Work>>(owner).is_none());
}

#[test]
fn replacement_and_controller_removal_clean_only_their_own_runs() {
    let mut app = app();
    app.add_state_tree_activity(
        Activity::<Work>::new("work")
            .with_collection()
            .with_marker::<Available>(),
    );
    let owner = spawn(&mut app, SINGLE);
    app.update();
    let definition = app
        .world()
        .get::<StateTreeInstance>(owner)
        .unwrap()
        .definition()
        .clone();
    let old = app
        .world()
        .get::<ActivityRuns<Work>>(owner)
        .unwrap()
        .iter()
        .next()
        .unwrap()
        .token();
    app.world_mut()
        .entity_mut(owner)
        .insert(StateTreeInstance::new(definition));
    app.update();
    let runs = app.world().get::<ActivityRuns<Work>>(owner).unwrap();
    assert_eq!(runs.len(), 1);
    assert!(runs.get(old).is_none());
    assert!(app.world().get::<Available>(owner).is_some());
    app.world_mut()
        .entity_mut(owner)
        .remove::<StateTreeInstance>();
    app.world_mut().flush();
    assert!(app.world().get::<Available>(owner).is_none());
    assert!(app.world().get::<ActivityRuns<Work>>(owner).is_none());
}

#[test]
fn owner_despawn_still_publishes_opted_in_cancellation() {
    let mut app = app();
    app.add_state_tree_activity(
        Activity::<Work>::new("work")
            .with_collection()
            .with_marker::<Available>()
            .with_lifecycle(),
    );
    let owner = spawn(&mut app, SINGLE);
    app.update();
    app.world_mut().despawn(owner);
    app.world_mut().flush();
    let messages = app
        .world()
        .resource::<Messages<StateTreeActivityLifecycle<Work>>>();
    assert!(MessageCursor::default().read(messages).any(|message| matches!(message, StateTreeActivityLifecycle::Cancelled { token } if token.entity() == owner)));
}

#[test]
fn raw_lifecycle_and_marker_activities_do_not_require_clone() {
    let mut app = app();
    app.add_state_tree_activity(
        Activity::<Other>::new("other")
            .with_marker::<Available>()
            .with_lifecycle(),
    );
    let owner = spawn(
        &mut app,
        r#"<state id="active"><ist:activity id="other" name="other"/></state>"#,
    );
    app.update();
    assert!(app.world().get::<Available>(owner).is_some());
    assert!(
        MessageCursor::default()
            .read(
                app.world()
                    .resource::<Messages<StateTreeActivityLifecycle<Other>>>()
            )
            .any(|message| matches!(message, StateTreeActivityLifecycle::Started { .. }))
    );
}

#[test]
#[should_panic(expected = "same retention policy")]
fn registrations_cannot_disagree_about_shared_collection_retention() {
    let mut app = app();
    app.add_state_tree_activity(Activity::<Work>::new("work").with_collection())
        .add_state_tree_activity(
            Activity::<Work>::new("alias")
                .with_collection_retention(ActivityCollectionRetention::KeepEmpty),
        );
}

#[test]
fn only_collection_and_lifecycle_fanout_clones_the_payload() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CLONES: AtomicUsize = AtomicUsize::new(0);

    #[derive(StateTreeInput)]
    struct Counted;

    impl Clone for Counted {
        fn clone(&self) -> Self {
            CLONES.fetch_add(1, Ordering::Relaxed);
            Self
        }
    }

    impl StateTreeActivity for Counted {
        type Output = ();
        type Error = ();
    }

    for (collection, lifecycle) in [(false, false), (true, false), (false, true), (true, true)] {
        CLONES.store(0, Ordering::Relaxed);
        let mut app = app();
        let mut activity = Activity::<Counted>::new("counted").with_marker::<Available>();
        if collection {
            activity = activity.with_collection();
        }
        if lifecycle {
            activity = activity.with_lifecycle();
        }
        app.add_state_tree_activity(activity);
        spawn(
            &mut app,
            r#"<state id="active"><ist:activity id="run" name="counted"/></state>"#,
        );
        app.update();
        assert_eq!(
            CLONES.load(Ordering::Relaxed),
            usize::from(collection && lifecycle)
        );
    }
}

#[test]
fn terminal_outcomes_release_bindings_without_requiring_a_state_exit() {
    for success in [true, false] {
        let mut app = app();
        app.add_state_tree_activity(
            Activity::<Work>::new("work")
                .with_collection()
                .with_marker::<Available>(),
        );
        let owner = spawn(&mut app, SINGLE);
        app.update();
        let token = app
            .world()
            .get::<ActivityRuns<Work>>(owner)
            .unwrap()
            .iter()
            .next()
            .unwrap()
            .token();
        app.world_mut()
            .run_system_once(move |mut outcomes: StateTreeActivityOutcomeWriter<Work>| {
                if success {
                    outcomes.complete(token, ());
                } else {
                    outcomes.fail(token, ());
                }
            })
            .unwrap();
        app.update();
        assert!(app.world().get::<Available>(owner).is_none());
        assert!(app.world().get::<ActivityRuns<Work>>(owner).is_none());
        assert_eq!(
            app.world()
                .get::<StateTreeInstance>(owner)
                .unwrap()
                .status(),
            &InstanceStatus::Running
        );
    }
}

#[cfg(feature = "persistence")]
#[test]
fn checkpoint_reconstructs_bindings_without_a_lifecycle_queue() {
    let mut app = app();
    app.add_state_tree_activity(
        Activity::<Work>::new("work")
            .with_collection()
            .with_marker::<Available>(),
    );
    let owner = spawn(&mut app, SINGLE);
    app.world_mut().entity_mut(owner).insert(PersistStateTree);
    app.update();
    let previous = app
        .world()
        .get::<ActivityRuns<Work>>(owner)
        .unwrap()
        .iter()
        .next()
        .unwrap()
        .token();
    app.world_mut()
        .get_mut::<ActivityRuns<Work>>(owner)
        .unwrap()
        .get_mut(previous)
        .unwrap()
        .amount = 99;
    let checkpoint = inslag_state_tree::persistence::Checkpoint::capture(app.world_mut()).unwrap();
    checkpoint.restore(app.world_mut()).unwrap();
    app.update();
    let (owner, runs) = app
        .world_mut()
        .query::<(Entity, &ActivityRuns<Work>)>()
        .single(app.world())
        .unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs.iter().next().unwrap().activity.amount, 7);
    assert_ne!(runs.iter().next().unwrap().token(), previous);
    assert!(app.world().get::<Available>(owner).is_some());
    assert!(
        !app.world()
            .contains_resource::<Messages<StateTreeActivityLifecycle<Work>>>()
    );
}
