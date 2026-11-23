use bevy_app::App;
use bevy_ecs::{entity::MapEntities, prelude::*, system::RunSystemOnce};
use inslag_persist::{AppSaveExt, Capture, Dirty, Error, SavePlugin, component};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[path = "support/requests.rs"]
mod requests;
use requests::{Requests, only_file};

const STRATEGIES: [Capture; 3] = [Capture::Snapshot, Capture::Changed, Capture::Dirty];

#[derive(Component, Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
struct Position {
    name: u32,
    value: u64,
}

#[derive(Component, Serialize, Deserialize, Debug, PartialEq, Eq)]
struct Label(String);

#[derive(Resource, Default, Serialize, Deserialize, Debug, PartialEq, Eq)]
struct Clock(u64);

#[derive(Component)]
struct Unrelated;

fn positions(world: &mut World) -> Vec<Position> {
    let mut values: Vec<_> = world.query::<&Position>().iter(world).cloned().collect();
    values.sort_by_key(|value| value.name);
    values
}

fn mark<T: Component>(world: &mut World, entity: Entity) {
    world
        .run_system_once(move |mut dirty: Dirty<T>| dirty.mark(entity))
        .unwrap();
}

#[test]
fn strategies_restore_identical_state_and_preserve_unrelated_data() {
    for strategy in STRATEGIES {
        let directory = tempfile::tempdir().unwrap();
        let mut app = app(directory.path(), strategy);
        let handle = app.new_request();
        let first = app
            .world_mut()
            .spawn((Position { name: 1, value: 10 }, Label("one".into())))
            .id();
        let second = app.world_mut().spawn(Position { name: 2, value: 20 }).id();
        let unrelated = app.world_mut().spawn(Unrelated).id();
        app.insert_resource(Clock(7));
        let initial = app.save_request().unwrap();
        assert_eq!(
            (initial.generation, initial.entities, initial.upserts),
            (1, 2, 4)
        );
        app.world_mut().get_mut::<Position>(first).unwrap().value = 11;
        mark::<Position>(app.world_mut(), first);
        app.world_mut().entity_mut(first).remove::<Label>();
        app.world_mut().despawn(second);
        app.world_mut().spawn(Position { name: 3, value: 30 });
        app.insert_resource(Clock(8));
        // Deletions remain pending even after Bevy's removal buffers expire.
        for _ in 0..5 {
            app.update();
        }
        let stats = app.save_request().unwrap();
        assert_eq!((stats.generation, stats.entities, stats.upserts), (2, 2, 3));
        let expected = positions(app.world_mut());
        app.world_mut().get_mut::<Position>(first).unwrap().value = 999;
        app.insert_resource(Clock(999));
        app.reload().unwrap();
        assert_eq!(positions(app.world_mut()), expected);
        let mut labels = app.world_mut().query::<&Label>();
        assert!(labels.iter(app.world()).next().is_none());
        assert_eq!(*app.world().resource::<Clock>(), Clock(8));
        assert!(app.world().get::<Unrelated>(unrelated).is_some());
        let mut fresh = self::app(directory.path(), strategy);
        fresh.load_request(handle).unwrap();
        assert_eq!(positions(fresh.world_mut()), expected);
        assert_eq!(*fresh.world().resource::<Clock>(), Clock(8));
        assert_eq!(fresh.save_request().unwrap().generation, 3);
    }
}

#[test]
fn changed_ticks_are_relative_to_commits_not_frames_or_world_clear_trackers() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path(), Capture::Changed);
    app.update();
    app.new_request();
    app.world_mut().remove_resource::<Clock>();
    let entity = app.world_mut().spawn(Position { name: 1, value: 1 }).id();
    assert_eq!(app.save_request().unwrap().upserts, 1);
    assert_eq!(app.save_request().unwrap().upserts, 0);
    app.world_mut().get_mut::<Position>(entity).unwrap().value = 2;
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(app.save_request().unwrap().upserts, 1);
    assert_eq!(app.save_request().unwrap().upserts, 0);
    // Direct world mutation immediately after save gets a fresh tick.
    app.world_mut().get_mut::<Position>(entity).unwrap().value = 3;
    assert_eq!(app.save_request().unwrap().upserts, 1);
}

#[test]
fn dirty_marks_coalesce_and_insert_replacements_are_automatic() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path(), Capture::Dirty);
    app.new_request();
    app.world_mut().remove_resource::<Clock>();
    let entity = app.world_mut().spawn(Position { name: 1, value: 1 }).id();
    app.save_request().unwrap();
    app.world_mut().get_mut::<Position>(entity).unwrap().value = 2;
    // Dirty is deliberately opt-in for in-place mutation.
    assert_eq!(app.save_request().unwrap().upserts, 0);
    for _ in 0..10 {
        mark::<Position>(app.world_mut(), entity);
    }
    assert_eq!(app.save_request().unwrap().upserts, 1);
    assert_eq!(app.save_request().unwrap().upserts, 0);
    app.world_mut()
        .entity_mut(entity)
        .insert(Position { name: 1, value: 3 });
    assert_eq!(app.save_request().unwrap().upserts, 1);
    app.world_mut().entity_mut(entity).remove::<Position>();
    app.world_mut()
        .entity_mut(entity)
        .insert(Position { name: 1, value: 4 });
    assert_eq!(app.save_request().unwrap().upserts, 1);
    app.reload().unwrap();
    assert_eq!(positions(app.world_mut()), [Position { name: 1, value: 4 }]);
}

#[test]
fn missing_resources_remove_the_persisted_value_and_restore_absence() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path(), Capture::Dirty);
    app.new_request();
    app.insert_resource(Clock(4));
    app.save_request().unwrap();
    app.world_mut().remove_resource::<Clock>();
    app.save_request().unwrap();
    app.insert_resource(Clock(99));
    app.reload().unwrap();
    assert!(!app.world().contains_resource::<Clock>());
}

#[test]
fn newly_registered_resources_default_but_saved_absence_stays_absent() {
    #[derive(Resource, Default, Serialize, Deserialize)]
    struct Settings {
        enabled: bool,
    }

    let directory = tempfile::tempdir().unwrap();
    let mut source = app(directory.path(), Capture::Changed);
    let handle = source.new_request();
    source.world_mut().remove_resource::<Clock>();
    source.save_request().unwrap();
    let mut target = app(directory.path(), Capture::Changed);
    target
        .persist_resource::<Settings>("settings")
        .insert_resource(Settings { enabled: true });
    target.load_request(handle).unwrap();
    assert!(!target.world().resource::<Settings>().enabled);
    assert!(!target.world().contains_resource::<Clock>());
    target.save_request().unwrap();
    let mut reloaded = app(directory.path(), Capture::Changed);
    reloaded.persist_resource::<Settings>("settings");
    reloaded.load_request(handle).unwrap();
    assert!(!reloaded.world().resource::<Settings>().enabled);
    assert!(!reloaded.world().contains_resource::<Clock>());
}

#[derive(Component, Serialize, Deserialize)]
struct Selected;

#[test]
fn deselected_attachment_is_deleted_on_commit_but_not_on_rollback() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new();
    app.add_plugins(SavePlugin::sqlite(directory.path()))
        .persist(component::<Position>("position"))
        .persist(
            component::<Label>("label")
                .attached()
                .filter::<With<Selected>>(),
        );
    app.new_request();
    let position = Position { name: 1, value: 10 };
    let entity = app
        .world_mut()
        .spawn((position.clone(), Label("attached".into()), Selected))
        .id();
    let second = Position { name: 2, value: 20 };
    let reselected = app
        .world_mut()
        .spawn((second.clone(), Label("reselected".into()), Selected))
        .id();
    let unowned = app
        .world_mut()
        .spawn((Label("unowned".into()), Selected))
        .id();
    let first = app.save_request().unwrap();
    assert_eq!((first.entities, first.upserts), (2, 4));
    let connection = Connection::open(only_file(directory.path())).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_commit BEFORE UPDATE ON meta
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();

    app.world_mut().entity_mut(entity).remove::<Selected>();
    app.world_mut().entity_mut(reselected).remove::<Selected>();
    assert!(app.save_request().is_err());
    let label_count = || {
        connection
            .query_row(
                "SELECT count(*) FROM records WHERE type_key = 'label'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
    };
    assert_eq!(label_count(), 2);
    connection
        .execute_batch("DROP TRIGGER reject_commit")
        .unwrap();
    app.world_mut().entity_mut(reselected).insert(Selected);
    let retry = app.save_request().unwrap();
    assert_eq!((retry.generation, retry.entities), (2, 2));
    assert!(app.world().get::<Label>(entity).is_some());

    app.reload().unwrap();
    assert_eq!(positions(app.world_mut()), [position, second]);
    assert_eq!(
        app.world_mut()
            .query_filtered::<&Label, With<Position>>()
            .single(app.world())
            .unwrap(),
        &Label("reselected".into())
    );
    assert_eq!(
        app.world().get::<Label>(unowned),
        Some(&Label("unowned".into()))
    );
}

#[test]
fn removing_a_filtered_attachment_keeps_the_owner_saved() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new();
    app.add_plugins(SavePlugin::sqlite(directory.path()))
        .persist(component::<Position>("position").capture(Capture::Dirty))
        .persist(
            component::<Label>("label")
                .attached()
                .filter::<With<Selected>>(),
        );
    app.new_request();
    let position = Position { name: 1, value: 10 };
    let entity = app
        .world_mut()
        .spawn((position.clone(), Label("attached".into()), Selected))
        .id();
    app.save_request().unwrap();

    app.world_mut().entity_mut(entity).remove::<Label>();
    let stats = app.save_request().unwrap();
    assert_eq!((stats.entities, stats.upserts), (1, 0));
    app.reload().unwrap();
    assert_eq!(positions(app.world_mut()), [position]);
    let mut labels = app.world_mut().query::<&Label>();
    assert!(labels.iter(app.world()).next().is_none());
}

#[test]
fn losing_the_last_owner_deletes_the_attachment() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new();
    app.add_plugins(SavePlugin::sqlite(directory.path()))
        .persist(component::<Position>("position"))
        .persist(
            component::<Label>("label")
                .attached()
                .filter::<With<Selected>>(),
        );
    app.new_request();
    let entity = app
        .world_mut()
        .spawn((
            Position { name: 1, value: 10 },
            Label("attached".into()),
            Selected,
        ))
        .id();
    app.save_request().unwrap();

    app.world_mut().entity_mut(entity).remove::<Position>();
    assert_eq!(app.save_request().unwrap().entities, 0);
    let records: i64 = Connection::open(only_file(directory.path()))
        .unwrap()
        .query_row("SELECT count(*) FROM records", [], |row| row.get(0))
        .unwrap();
    assert_eq!(records, 0);
}

#[test]
fn filtered_membership_changes_are_detected_without_marking_payloads() {
    for strategy in STRATEGIES {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new();
        app.add_plugins(SavePlugin::sqlite(directory.path()))
            .persist(
                component::<Position>("position")
                    .filter::<With<Selected>>()
                    .capture(strategy),
            )
            .persist(component::<Selected>("selected"));
        app.new_request();
        let entity = app.world_mut().spawn(Position { name: 1, value: 10 }).id();
        assert_eq!(app.save_request().unwrap().entities, 0);
        app.world_mut().entity_mut(entity).insert(Selected);
        let stats = app.save_request().unwrap();
        assert_eq!((stats.entities, stats.upserts), (1, 2));
        app.world_mut().entity_mut(entity).remove::<Selected>();
        assert_eq!(app.save_request().unwrap().entities, 0);
        let records: i64 = Connection::open(only_file(directory.path()))
            .unwrap()
            .query_row("SELECT count(*) FROM records", [], |row| row.get(0))
            .unwrap();
        assert_eq!(records, 0);
        app.world_mut().entity_mut(entity).insert(Selected);
        app.save_request().unwrap();
        app.reload().unwrap();
        assert_eq!(
            positions(app.world_mut()),
            [Position { name: 1, value: 10 }]
        );
        assert_eq!(
            app.world_mut()
                .query::<&Selected>()
                .iter(app.world())
                .count(),
            1
        );
    }
}

#[test]
fn disabled_entities_are_persisted_consistently_by_every_strategy() {
    use bevy_ecs::entity_disabling::Disabled;
    for strategy in STRATEGIES {
        let directory = tempfile::tempdir().unwrap();
        let mut app = app(directory.path(), strategy);
        app.new_request();
        app.world_mut().remove_resource::<Clock>();
        let entity = app
            .world_mut()
            .spawn((Position { name: 1, value: 4 }, Disabled))
            .id();
        assert_eq!(app.save_request().unwrap().upserts, 1);
        app.world_mut().get_mut::<Position>(entity).unwrap().value = 5;
        mark::<Position>(app.world_mut(), entity);
        assert_eq!(app.save_request().unwrap().upserts, 1);
        app.reload().unwrap();
        assert_eq!(positions(app.world_mut()), [Position { name: 1, value: 5 }]);
    }
}

#[derive(Component, Serialize, Deserialize)]
struct Link {
    #[entities]
    #[serde(with = "inslag_persist::entity")]
    target: Entity,
}

#[derive(Resource, Serialize, Deserialize, MapEntities)]
struct Focus {
    #[entities]
    #[serde(with = "inslag_persist::entity")]
    target: Entity,
}

impl Default for Focus {
    fn default() -> Self {
        Self {
            target: Entity::PLACEHOLDER,
        }
    }
}

fn graph_app(path: &Path, strategy: Capture) -> App {
    let mut app = app(path, strategy);
    app.persist(component::<Link>("link").capture(strategy))
        .persist_resource::<Focus>("focus");
    app
}

fn app(path: &Path, strategy: Capture) -> App {
    let mut app = App::new();
    app.add_plugins(SavePlugin::sqlite(path))
        .persist(component::<Position>("position").capture(strategy))
        .persist(component::<Label>("label").capture(strategy))
        .persist_resource::<Clock>("clock");
    app
}

#[test]
fn cyclic_entity_graphs_and_resource_references_remap_across_apps() {
    for strategy in STRATEGIES {
        let directory = tempfile::tempdir().unwrap();
        let mut writer = graph_app(directory.path(), strategy);
        let handle = writer.new_request();
        let a = writer
            .world_mut()
            .spawn(Position { name: 1, value: 10 })
            .id();
        let b = writer
            .world_mut()
            .spawn(Position { name: 2, value: 20 })
            .id();
        writer.world_mut().entity_mut(a).insert(Link { target: b });
        writer.world_mut().entity_mut(b).insert(Link { target: a });
        writer.insert_resource(Focus { target: a });
        writer.save_request().unwrap();
        let mut reader = graph_app(directory.path(), strategy);
        for _ in 0..100 {
            reader.world_mut().spawn(Unrelated);
        }
        reader.load_request(handle).unwrap();
        let mut query = reader.world_mut().query::<(Entity, &Position, &Link)>();
        let nodes: Vec<_> = query
            .iter(reader.world())
            .map(|(e, p, l)| (e, p.name, l.target))
            .collect();
        let a = nodes.iter().find(|(_, name, _)| *name == 1).unwrap();
        let b = nodes.iter().find(|(_, name, _)| *name == 2).unwrap();
        assert_eq!((a.2, b.2), (b.0, a.0));
        assert_eq!(reader.world().resource::<Focus>().target, a.0);
        let path = only_file(directory.path());
        let before: Vec<u8> = Connection::open(&path)
            .unwrap()
            .query_row(
                "SELECT payload FROM records WHERE type_key = 'focus'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        reader.save_request().unwrap();
        let after: Vec<u8> = Connection::open(&path)
            .unwrap()
            .query_row(
                "SELECT payload FROM records WHERE type_key = 'focus'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(before, after);
    }
}

#[test]
fn untouched_references_prevent_incremental_target_deletion() {
    for strategy in [Capture::Changed, Capture::Dirty] {
        let directory = tempfile::tempdir().unwrap();
        let mut writer = graph_app(directory.path(), strategy);
        let handle = writer.new_request();
        writer.world_mut().remove_resource::<Focus>();
        let a = writer
            .world_mut()
            .spawn(Position { name: 1, value: 1 })
            .id();
        let b = writer
            .world_mut()
            .spawn(Position { name: 2, value: 2 })
            .id();
        writer.world_mut().entity_mut(a).insert(Link { target: b });
        writer.save_request().unwrap();
        writer.world_mut().despawn(b);
        assert!(writer.save_request().is_err());
        let mut reader = graph_app(directory.path(), strategy);
        reader.load_request(handle).unwrap();
        assert_eq!(positions(reader.world_mut()).len(), 2);
        writer.world_mut().entity_mut(a).remove::<Link>();
        let retry = writer.save_request().unwrap();
        assert_eq!((retry.generation, retry.entities), (2, 1));
        reader.reload().unwrap();
        assert_eq!(
            positions(reader.world_mut()),
            [Position { name: 1, value: 1 }]
        );
        let mut links = reader.world_mut().query::<&Link>();
        assert!(links.iter(reader.world()).next().is_none());
    }
}

#[test]
fn references_outside_selection_fail_without_creating_a_save() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = graph_app(directory.path(), Capture::Snapshot);
    app.new_request();
    app.world_mut().remove_resource::<Focus>();
    let outside = app.world_mut().spawn(Unrelated).id();
    app.world_mut().spawn(Link { target: outside });
    assert!(
        app.save_request()
            .is_err_and(|error| matches!(*error, Error::Encode { .. }))
    );
    assert!(
        std::fs::read_dir(directory.path())
            .unwrap()
            .next()
            .is_none()
    );
    app.world_mut()
        .entity_mut(outside)
        .insert(Position { name: 1, value: 2 });
    assert_eq!(app.save_request().unwrap().generation, 1);
}

#[test]
fn decode_failure_preserves_the_entire_live_scope() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = app(directory.path(), Capture::Changed);
    let handle = writer.new_request();
    writer.world_mut().spawn(Position { name: 1, value: 1 });
    writer.insert_resource(Clock(1));
    writer.save_request().unwrap();
    let connection = Connection::open(only_file(directory.path())).unwrap();
    connection
        .execute(
            "UPDATE records SET payload = ?1 WHERE type_key = 'clock'",
            [b"invalid".to_vec()],
        )
        .unwrap();
    let mut app = app(directory.path(), Capture::Changed);
    let entity = app.world_mut().spawn(Position { name: 1, value: 42 }).id();
    app.insert_resource(Clock(42));
    assert!(
        app.load_request(handle)
            .is_err_and(|error| matches!(*error, Error::Decode { .. }))
    );
    assert_eq!(app.world().get::<Position>(entity).unwrap().value, 42);
    assert_eq!(*app.world().resource::<Clock>(), Clock(42));
}

#[test]
fn dangling_reference_decode_fails_before_entities_are_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = graph_app(directory.path(), Capture::Dirty);
    let handle = writer.new_request();
    writer.world_mut().remove_resource::<Focus>();
    let entity = writer
        .world_mut()
        .spawn(Position { name: 1, value: 1 })
        .id();
    writer
        .world_mut()
        .entity_mut(entity)
        .insert(Link { target: entity });
    writer.save_request().unwrap();
    Connection::open(only_file(directory.path()))
        .unwrap()
        .execute(
            "UPDATE records SET payload = ?1 WHERE type_key = 'link'",
            [br#"{"target":999999}"#.to_vec()],
        )
        .unwrap();
    let mut app = graph_app(directory.path(), Capture::Dirty);
    let entity = app.world_mut().spawn(Position { name: 1, value: 42 }).id();
    assert!(
        app.load_request(handle)
            .is_err_and(|error| matches!(*error, Error::Decode { .. }))
    );
    assert!(app.world().get::<Position>(entity).is_some());
}

#[test]
fn sqlite_failure_rolls_back_prior_writes_and_preserves_pending_changes() {
    for strategy in STRATEGIES {
        let directory = tempfile::tempdir().unwrap();
        let mut writer = app(directory.path(), strategy);
        let handle = writer.new_request();
        writer.world_mut().remove_resource::<Clock>();
        let entity = writer
            .world_mut()
            .spawn((Position { name: 1, value: 1 }, Label("old".into())))
            .id();
        writer.save_request().unwrap();
        let connection = Connection::open(only_file(directory.path())).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER reject_label BEFORE UPDATE ON records WHEN NEW.type_key = 'label'
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
            )
            .unwrap();
        writer
            .world_mut()
            .get_mut::<Position>(entity)
            .unwrap()
            .value = 2;
        writer.world_mut().get_mut::<Label>(entity).unwrap().0 = "new".into();
        mark::<Position>(writer.world_mut(), entity);
        mark::<Label>(writer.world_mut(), entity);
        let error = writer.save_request().unwrap_err();
        assert!(matches!(*error, Error::Io(_)));
        assert!(
            std::error::Error::source(error.as_ref())
                .is_some_and(|source| source.is::<rusqlite::Error>())
        );
        let mut reader = app(directory.path(), strategy);
        reader.load_request(handle).unwrap();
        assert_eq!(
            positions(reader.world_mut()),
            [Position { name: 1, value: 1 }]
        );
        connection
            .execute_batch("DROP TRIGGER reject_label")
            .unwrap();
        let retry = writer.save_request().unwrap();
        assert_eq!((retry.generation, retry.upserts), (2, 2));
        reader.reload().unwrap();
        assert_eq!(
            positions(reader.world_mut()),
            [Position { name: 1, value: 2 }]
        );
        assert_eq!(
            reader
                .world_mut()
                .query::<&Label>()
                .single(reader.world())
                .unwrap()
                .0,
            "new"
        );
    }
}

#[derive(Component, Serialize, Deserialize)]
struct Fallible {
    #[serde(serialize_with = "serialize_fallible")]
    value: u64,
}

fn serialize_fallible<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    if *value == u64::MAX {
        return Err(serde::ser::Error::custom("injected encoding failure"));
    }
    serializer.serialize_u64(*value)
}

#[test]
fn encoding_failure_does_not_acknowledge_dirty_marks() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new();
    app.add_plugins(SavePlugin::sqlite(directory.path()))
        .persist(component::<Fallible>("fallible").capture(Capture::Dirty));
    app.new_request();
    let entity = app.world_mut().spawn(Fallible { value: 1 }).id();
    app.save_request().unwrap();
    app.world_mut().get_mut::<Fallible>(entity).unwrap().value = u64::MAX;
    mark::<Fallible>(app.world_mut(), entity);
    assert!(
        app.save_request()
            .is_err_and(|error| matches!(*error, Error::Encode { .. }))
    );
    app.world_mut().get_mut::<Fallible>(entity).unwrap().value = 2;
    assert_eq!(app.save_request().unwrap().upserts, 1);
    app.reload().unwrap();
    assert_eq!(
        app.world_mut()
            .query::<&Fallible>()
            .single(app.world())
            .unwrap()
            .value,
        2
    );
}

#[test]
fn concurrent_writer_requires_reloading_before_incremental_save() {
    let directory = tempfile::tempdir().unwrap();
    let mut a = app(directory.path(), Capture::Changed);
    let handle = a.new_request();
    a.world_mut().spawn(Position { name: 1, value: 1 });
    a.save_request().unwrap();
    let mut b = app(directory.path(), Capture::Changed);
    b.load_request(handle).unwrap();
    a.save_request().unwrap();
    assert!(b.save_request().is_err_and(|error| matches!(
        *error,
        Error::Conflict {
            expected: 1,
            actual: 2
        }
    )));
    b.reload().unwrap();
    assert_eq!(b.save_request().unwrap().generation, 3);
}

#[test]
fn wrong_manifest_and_foreign_databases_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = app(directory.path(), Capture::Snapshot);
    let handle = writer.new_request();
    writer.save_request().unwrap();
    let mut different = App::new();
    different
        .add_plugins(SavePlugin::sqlite(directory.path()))
        .persist(component::<Position>("renamed"));
    assert!(
        different
            .load_request(handle)
            .is_err_and(|error| matches!(*error, Error::Manifest))
    );
    assert!(
        different
            .save_request()
            .is_err_and(|error| matches!(*error, Error::Closed))
    );
    let path = only_file(directory.path());
    let foreign = tempfile::tempdir().unwrap();
    let foreign_path = foreign.path().join(path.file_name().unwrap());
    let connection = Connection::open(foreign_path).unwrap();
    connection
        .execute_batch("CREATE TABLE important(value INTEGER); INSERT INTO important VALUES (123);")
        .unwrap();
    let mut reader = app(foreign.path(), Capture::Snapshot);
    assert!(
        reader
            .load_request(handle)
            .is_err_and(|error| matches!(*error, Error::Format))
    );
    let value: i64 = connection
        .query_row("SELECT value FROM important", [], |row| row.get(0))
        .unwrap();
    assert_eq!(value, 123);
}

#[test]
fn mismatched_manifest_roles_preserve_live_state() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = app(directory.path(), Capture::Changed);
    let handle = writer.new_request();
    writer.world_mut().spawn(Position { name: 1, value: 10 });
    writer.insert_resource(Clock(7));
    writer.save_request().unwrap();
    Connection::open(only_file(directory.path()))
        .unwrap()
        .execute("UPDATE types SET kind = 0 WHERE key = 'position'", [])
        .unwrap();

    let mut app = app(directory.path(), Capture::Changed);
    let entity = app.world_mut().spawn(Position { name: 1, value: 42 }).id();
    app.insert_resource(Clock(42));
    let count = app.world().entities().len();
    assert!(
        app.load_request(handle)
            .is_err_and(|error| matches!(*error, Error::Manifest))
    );
    assert_eq!(app.world().get::<Position>(entity).unwrap().value, 42);
    assert_eq!(app.world().resource::<Clock>().0, 42);
    assert_eq!(app.world().entities().len(), count);
}

#[test]
fn missing_file_load_does_not_create_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("missing");
    let mut app = app(&path, Capture::Snapshot);
    let handle = app.new_request();
    app.close_request();
    assert!(
        app.load_request(handle)
            .is_err_and(|error| matches!(*error, Error::NotFound))
    );
    assert!(!path.exists());
}

#[test]
fn failed_load_blocks_writes_until_a_successful_reload() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = app(directory.path(), Capture::Changed);
    let handle = writer.new_request();
    writer.insert_resource(Clock(7));
    writer.save_request().unwrap();
    writer.close_request();
    let connection = Connection::open(only_file(directory.path())).unwrap();
    connection
        .execute(
            "UPDATE records SET payload = X'00' WHERE type_key = 'clock'",
            [],
        )
        .unwrap();
    assert!(
        writer
            .load_request(handle)
            .is_err_and(|error| matches!(*error, Error::Decode { .. }))
    );
    for _ in 0..3 {
        assert!(
            writer
                .save_request()
                .is_err_and(|error| matches!(*error, Error::Closed))
        );
    }
    let generation: i64 = connection
        .query_row("SELECT generation FROM meta", [], |row| row.get(0))
        .unwrap();
    assert_eq!(generation, 1);
    connection
        .execute(
            "UPDATE records SET payload = ?1 WHERE type_key = 'clock'",
            [b"9".to_vec()],
        )
        .unwrap();
    writer.load_request(handle).unwrap();
    assert_eq!(writer.world().resource::<Clock>().0, 9);
    assert_eq!(writer.save_request().unwrap().generation, 2);
}

#[test]
fn corrupt_orphan_record_is_rejected_even_if_foreign_keys_were_disabled() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path(), Capture::Snapshot);
    app.new_request();
    app.save_request().unwrap();
    let connection = Connection::open(only_file(directory.path())).unwrap();
    connection.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    connection
        .execute(
            "INSERT INTO records VALUES (123, 'position', ?1)",
            params![br#"{"name":1,"value":2}"#.to_vec()],
        )
        .unwrap();
    assert!(
        app.reload()
            .is_err_and(|error| matches!(*error, Error::InvalidData(_)))
    );
}

#[test]
fn empty_file_is_a_failed_load_not_a_missing_save() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = app(directory.path(), Capture::Snapshot);
    let handle = writer.new_request();
    writer.save_request().unwrap();
    let path = only_file(directory.path());
    let other = tempfile::tempdir().unwrap();
    let empty = other.path().join(path.file_name().unwrap());
    std::fs::write(&empty, []).unwrap();
    let mut reader = app(other.path(), Capture::Snapshot);
    assert!(
        reader
            .load_request(handle)
            .is_err_and(|error| matches!(*error, Error::Format))
    );
    assert_eq!(std::fs::metadata(empty).unwrap().len(), 0);
}

#[test]
fn filesystem_failures_preserve_the_io_source() {
    let mut app = app(Path::new("invalid\0path"), Capture::Snapshot);
    let error = app.list_request().unwrap_err();
    assert!(matches!(*error, Error::Io(_)));
    let source = std::error::Error::source(error.as_ref())
        .and_then(|source| source.downcast_ref::<std::io::Error>())
        .unwrap();
    assert_eq!(source.kind(), std::io::ErrorKind::InvalidInput);
}
