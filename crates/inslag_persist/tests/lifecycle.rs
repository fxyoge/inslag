use bevy_app::{App, AppExit, Update};
use bevy_ecs::{prelude::*, system::RunSystemOnce};
use bevy_time::{Real, Time};
use inslag_persist::{
    AppSaveExt, Capture, Dirty, Error, LoadOutcome, SaveCompleted, SaveInfo, SaveOpened,
    SavePlugin, SaveRequest, SaveStatus, component,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{Duration, UNIX_EPOCH},
};
mod support;
use support::settle;
#[path = "support/requests.rs"]
mod requests;
use requests::{Requests, only_file};

#[derive(Resource, Default, Serialize, Deserialize)]
struct Counter(u64);

#[derive(Resource)]
struct Ready(bool);

#[derive(Component, Serialize, Deserialize)]
struct Peer {
    #[entities]
    #[serde(with = "inslag_persist::entity")]
    target: Entity,
}

#[derive(Component)]
struct Transient;

fn fresh(status: Res<SaveStatus>) -> bool {
    matches!(status.load(), Some(Ok(LoadOutcome::Fresh)))
}

fn initialize(mut commands: Commands) {
    commands.insert_resource(Counter(10));
    let a = commands.spawn_empty().id();
    let b = commands.spawn_empty().id();
    commands.entity(a).insert(Peer { target: b });
    commands.entity(b).insert(Peer { target: a });
}

fn app(path: &Path) -> App {
    let mut app = App::new();
    app.init_resource::<Time<Real>>()
        .insert_resource(Ready(true))
        .add_plugins(
            SavePlugin::sqlite(path)
                .autosave(Duration::from_secs(1))
                .enabled_when(|ready: Res<Ready>| ready.0),
        )
        .persist_resource::<Counter>("counter")
        .persist(component::<Peer>("peer").capture(Capture::Changed))
        .add_systems(SaveOpened, initialize.run_if(fresh));
    app
}

fn tick(app: &mut App, duration: Duration) {
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(duration);
    app.update();
    settle(app);
}

fn request(app: &mut App, request: SaveRequest) {
    app.world_mut().write_message(request);
    app.update();
    settle(app);
}

fn generation(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT generation FROM meta", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn plugin_restores_before_save_opened_and_autosaves_deferred_mutations() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = app(directory.path());
    let transient = first.world_mut().spawn(Transient).id();
    first.update();
    let handle = first.new_request();
    assert!(first.world().resource::<SaveStatus>().can_save());
    assert!(first.list_request().unwrap().is_empty());
    first.add_systems(Update, |mut commands: Commands| {
        commands.insert_resource(Counter(42));
    });
    tick(&mut first, Duration::from_secs(1));
    assert_eq!(generation(&only_file(directory.path())), 1);
    assert!(first.world().get::<Transient>(transient).is_some());
    let mut second = app(directory.path());
    second.add_systems(SaveOpened, |counter: Res<Counter>| {
        assert_eq!(counter.0, 42)
    });
    second.update();
    second.load_request(handle).unwrap();
    assert!(matches!(
        second.world().resource::<SaveStatus>().load(),
        Some(Ok(LoadOutcome::Loaded))
    ));
    let pairs: Vec<_> = second
        .world_mut()
        .query::<(Entity, &Peer)>()
        .iter(second.world())
        .map(|(entity, peer)| (entity, peer.target))
        .collect();
    assert_eq!(pairs.len(), 2); // fresh initialization did not run again
    for (entity, target) in pairs {
        assert_eq!(second.world().get::<Peer>(target).unwrap().target, entity);
    }
    assert_eq!(second.world().resource::<Counter>().0, 42);
}

#[test]
fn readiness_and_real_elapsed_deadlines_control_saves_without_frame_retries() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saves");
    let mut app = app(&path);
    app.update();
    app.new_request();
    app.world_mut().resource_mut::<Ready>().0 = false;
    tick(&mut app, Duration::from_secs(1));
    assert!(!path.exists());
    assert!(
        app.world()
            .resource::<SaveStatus>()
            .last_save()
            .unwrap()
            .is_err()
    );
    app.world_mut().resource_mut::<Ready>().0 = true;
    for _ in 0..100 {
        app.update();
    }
    assert!(!path.exists());
    tick(&mut app, Duration::from_secs(10));
    let path = only_file(&path);
    assert_eq!(generation(&path), 1); // one commit, not ten catch-up commits
    for _ in 0..100 {
        app.update();
    }
    assert_eq!(generation(&path), 1);
}

#[test]
fn failed_load_stays_protected_across_autosave_deadlines_and_explicit_requests() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = app(directory.path());
    first.update();
    let handle = first.new_request();
    request(&mut first, SaveRequest::Save);
    let path = only_file(directory.path());
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE records SET payload = X'00' WHERE type_key = 'counter'",
            [],
        )
        .unwrap();
    let mut second = app(directory.path());
    second.insert_resource(Counter(99));
    second.update();
    assert!(second.load_request(handle).is_err());
    assert!(
        second
            .world()
            .resource::<SaveStatus>()
            .load()
            .unwrap()
            .is_err()
    );
    for _ in 0..10 {
        tick(&mut second, Duration::from_secs(1));
    }
    assert!(
        second
            .save_request()
            .is_err_and(|error| matches!(*error, Error::Closed))
    );
    assert_eq!(generation(&path), 1);
    assert_eq!(second.world().resource::<Counter>().0, 99);
    assert!(!second.world().resource::<SaveStatus>().can_save());
    connection
        .execute(
            "UPDATE records SET payload = ?1 WHERE type_key = 'counter'",
            [b"23".to_vec()],
        )
        .unwrap();
    second.load_request(handle).unwrap();
    assert_eq!(second.world().resource::<Counter>().0, 23);
    tick(&mut second, Duration::from_secs(1));
    assert_eq!(generation(&path), 2);
}

#[test]
fn explicit_session_switches_isolate_saves_and_preserve_unrelated_entities() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    let transient = app.world_mut().spawn(Transient).id();
    app.update();
    let a = app.new_request();
    app.world_mut().resource_mut::<Counter>().0 = 31;
    app.save_request().unwrap();
    app.close_request();
    let b = app.new_request();
    assert_ne!(a, b);
    assert_eq!(app.world().resource::<Counter>().0, 10); // fresh, not copied from A
    app.world_mut().resource_mut::<Counter>().0 = 52;
    let listed = app.list_request().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].handle, a);
    assert_eq!(app.world().resource::<Counter>().0, 52);
    app.save_request().unwrap();
    assert_eq!(app.list_request().unwrap().len(), 2);
    app.close_request();
    app.load_request(a).unwrap();
    assert_eq!(app.world().resource::<Counter>().0, 31);
    app.close_request();
    app.load_request(b).unwrap();
    assert_eq!(app.world().resource::<Counter>().0, 52);
    app.close_request();
    assert_eq!(app.world().resource::<Counter>().0, 0);
    assert_eq!(
        app.world_mut().query::<&Peer>().iter(app.world()).count(),
        0
    );
    assert!(app.world().get::<Transient>(transient).is_some());
    assert!(
        app.save_request()
            .is_err_and(|error| matches!(*error, Error::Closed))
    );
    app.load_request(b).unwrap();
    assert_eq!(app.world().resource::<Counter>().0, 52);
}

#[test]
fn failed_save_keeps_session_and_pending_changes_for_retry() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    let handle = app.new_request();
    app.save_request().unwrap();
    let path = only_file(directory.path());
    app.world_mut().resource_mut::<Counter>().0 = 88;
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_commit BEFORE UPDATE ON meta BEGIN SELECT RAISE(ABORT, 'failure'); END;").unwrap();
    assert!(app.save_request().is_err());
    assert_eq!(app.world().resource::<SaveStatus>().handle(), Some(handle));
    assert_eq!(app.world().resource::<Counter>().0, 88);
    assert_eq!(generation(&path), 1);
    // Frames before the next deadline must not retry the failed operation.
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(generation(&path), 1);
    connection
        .execute_batch("DROP TRIGGER reject_commit")
        .unwrap();
    app.save_request().unwrap();
    assert_eq!(generation(&path), 2);
    let mut restored = self::app(directory.path());
    restored.load_request(handle).unwrap();
    assert_eq!(restored.world().resource::<Counter>().0, 88);
}

#[test]
fn new_and_load_reject_an_open_session_without_writing() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    let target = app.new_request();
    app.save_request().unwrap();
    app.close_request();
    let active = app.new_request();
    app.world_mut().resource_mut::<Counter>().0 = 77;
    assert!(matches!(app.request(SaveRequest::New),
        SaveCompleted::New(Err(error)) if matches!(*error, Error::AlreadyOpen)));
    assert!(
        app.load_request(target)
            .is_err_and(|error| matches!(*error, Error::AlreadyOpen))
    );
    assert_eq!(app.world().resource::<SaveStatus>().handle(), Some(active));
    assert!(app.world().resource::<SaveStatus>().can_save());
    assert_eq!(app.world().resource::<Counter>().0, 77);
    app.close_request();
    let listed = app.list_request().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].handle, target);
    assert_eq!(generation(&only_file(directory.path())), 1);
}

#[test]
fn exit_passes_through_without_saving_an_open_session() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saves");
    let mut app = app(&path);
    app.update();
    app.new_request();
    app.world_mut().resource_mut::<Counter>().0 = 64;
    app.world_mut().write_message(AppExit::Success);
    app.update();
    assert_eq!(app.should_exit(), Some(AppExit::Success));
    assert!(app.world().resource::<SaveStatus>().last_save().is_none());
    assert!(!path.exists());
}

#[test]
fn a_batch_reports_busy_requests_without_silently_discarding_them() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    let mut cursor = app
        .world()
        .resource::<Messages<SaveCompleted>>()
        .get_cursor_current();
    app.world_mut().write_message(SaveRequest::New);
    app.world_mut().write_message(SaveRequest::Save);
    app.world_mut().write_message(SaveRequest::Close);
    app.update();
    let results: Vec<_> = cursor.read(app.world().resource()).collect();
    assert!(matches!(results.as_slice(), [
        SaveCompleted::New(Ok(_)),
        SaveCompleted::Save { automatic: false, result: Err(save) },
        SaveCompleted::Close(Err(close)),
    ] if matches!(**save, Error::Busy) && matches!(**close, Error::Busy)));
    assert!(app.list_request().unwrap().is_empty());
    assert_eq!(app.save_request().unwrap().generation, 1);
}

#[test]
fn saving_during_an_open_is_busy_and_is_not_retried() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saves");
    let mut app = app(&path);
    app.world_mut().resource_mut::<Ready>().0 = false;
    app.world_mut().write_message(SaveRequest::New);
    app.update();
    assert!(app.world().resource::<SaveStatus>().is_loading());
    assert!(
        app.save_request()
            .is_err_and(|error| matches!(*error, Error::Busy))
    );
    assert!(app.list_request().unwrap().is_empty());
    app.world_mut().resource_mut::<Ready>().0 = true;
    settle(&mut app);
    assert!(app.world().resource::<SaveStatus>().can_save());
    assert!(!path.exists());
    assert_eq!(app.save_request().unwrap().generation, 1);
}

#[test]
fn exit_without_opening_a_session_does_not_create_a_save() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saves");
    let mut app = App::new();
    app.add_plugins(SavePlugin::sqlite(&path));
    app.update();
    assert!(app.world().resource::<SaveStatus>().handle().is_none());
    assert!(!path.exists());
    assert!(app.list_request().unwrap().is_empty());
    app.world_mut().write_message(AppExit::Success);
    app.update();
    assert!(!path.exists());
    assert_eq!(app.should_exit(), Some(AppExit::Success));
}

#[test]
fn a_due_autosave_does_not_duplicate_an_explicit_save() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    app.update();
    app.new_request();
    app.world_mut().write_message(SaveRequest::Save);
    tick(&mut app, Duration::from_secs(1));
    assert_eq!(generation(&only_file(directory.path())), 1);
}

#[test]
fn list_returns_creation_metadata_and_ignores_unfinished_databases() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = app(directory.path());
    let committed = writer.new_request();
    writer.world_mut().resource_mut::<Counter>().0 = 81;
    writer.save_request().unwrap();
    let unfinished = directory
        .path()
        .join("00000000000000000000000000000000.sqlite");
    let connection = Connection::open(unfinished).unwrap();
    connection
        .execute_batch(
            "PRAGMA application_id = 1195984705; PRAGMA user_version = 2;
         CREATE TABLE meta (singleton INTEGER PRIMARY KEY, generation INTEGER, created_at_ms INTEGER);
         INSERT INTO meta VALUES (1, 0, 0);",
        )
        .unwrap();
    std::fs::write(directory.path().join("notes.txt"), b"not a save").unwrap();
    let mut reader = app(directory.path());
    let saves = reader.list_request().unwrap();
    assert_eq!(saves.len(), 1);
    assert_eq!(saves[0].handle, committed);
    assert!(saves[0].created_at > UNIX_EPOCH);
    assert_eq!(reader.world().resource::<SaveStatus>().handle(), None);
    reader.load_request(saves[0].handle).unwrap();
    assert_eq!(reader.world().resource::<Counter>().0, 81);
}

#[test]
fn resaving_preserves_the_creation_timestamp() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = app(directory.path());
    let handle = writer.new_request();
    writer.save_request().unwrap();
    let connection = Connection::open(only_file(directory.path())).unwrap();
    connection
        .execute("UPDATE meta SET created_at_ms = 123456", [])
        .unwrap();
    writer.close_request();
    writer.load_request(handle).unwrap();
    writer.save_request().unwrap();
    assert_eq!(
        writer.list_request().unwrap(),
        [SaveInfo {
            handle,
            created_at: UNIX_EPOCH + Duration::from_millis(123456),
        }]
    );
}

#[derive(Component, Serialize, Deserialize)]
struct DirtyValue(u64);

#[derive(Component, Serialize, Deserialize)]
struct ChangedValue(u64);

#[test]
fn edits_during_sqlite_commit_survive_success_and_rollback() {
    for abort in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new();
        app.add_plugins(SavePlugin::sqlite(directory.path()))
            .persist_resource::<Counter>("counter")
            .persist(component::<DirtyValue>("dirty").capture(Capture::Dirty))
            .persist(component::<ChangedValue>("changed").capture(Capture::Changed));
        app.update();
        app.new_request();
        let world = app.world_mut();
        let edited = world.spawn((DirtyValue(1), ChangedValue(4))).id();
        let readded = world.spawn(DirtyValue(7)).id();
        let captured_only = world.spawn(DirtyValue(10)).id();
        assert_eq!(app.save_request().unwrap().generation, 1);
        let path = only_file(directory.path());
        let connection = Connection::open(&path).unwrap();
        if abort {
            connection.execute_batch("CREATE TRIGGER reject_commit BEFORE UPDATE ON meta BEGIN SELECT RAISE(ABORT, 'failure'); END;").unwrap();
        }
        connection.execute_batch("BEGIN IMMEDIATE").unwrap();
        let world = app.world_mut();
        edit_dirty(world, edited, 2);
        world.get_mut::<ChangedValue>(edited).unwrap().0 = 5;
        world.entity_mut(readded).remove::<DirtyValue>();
        edit_dirty(world, captured_only, 11);
        world.insert_resource(Counter(20));

        world.write_message(SaveRequest::Save);
        app.update();
        assert!(app.world().resource::<SaveStatus>().is_saving());

        let world = app.world_mut();
        edit_dirty(world, edited, 3);
        world.get_mut::<ChangedValue>(edited).unwrap().0 = 6;
        world.entity_mut(readded).insert(DirtyValue(8));
        world.remove_resource::<Counter>();
        app.update();
        assert!(app.world().resource::<SaveStatus>().is_saving());
        assert_eq!(generation(&path), 1);

        connection.execute_batch("COMMIT").unwrap();
        settle(&mut app);
        assert_eq!(
            app.world()
                .resource::<SaveStatus>()
                .last_save()
                .unwrap()
                .is_err(),
            abort
        );
        if abort {
            assert_eq!(saved_values(&connection, "dirty"), [1, 7, 10]);
            assert_eq!(saved_values(&connection, "changed"), [4]);
            assert_eq!(saved_values(&connection, "counter"), [0]);
            connection
                .execute_batch("DROP TRIGGER reject_commit")
                .unwrap();
        } else {
            assert_eq!(saved_values(&connection, "dirty"), [2, 11]);
            assert_eq!(saved_values(&connection, "changed"), [5]);
            assert_eq!(saved_values(&connection, "counter"), [20]);
        }

        request(&mut app, SaveRequest::Save);
        assert_eq!(generation(&path), if abort { 2 } else { 3 });
        assert_eq!(saved_values(&connection, "dirty"), [3, 8, 11]);
        assert_eq!(saved_values(&connection, "changed"), [6]);
        assert!(saved_values(&connection, "counter").is_empty());
    }
}

#[test]
fn close_during_a_commit_is_busy_then_closes_without_writing_newer_edits() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    app.update();
    app.new_request();
    request(&mut app, SaveRequest::Save);
    let path = only_file(directory.path());
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    app.world_mut().write_message(SaveRequest::Save);
    app.update();

    app.world_mut().resource_mut::<Counter>().0 = 55;
    assert!(matches!(app.request(SaveRequest::Close),
        SaveCompleted::Close(Err(error)) if matches!(*error, Error::Busy)));
    assert!(app.world().resource::<SaveStatus>().is_saving());
    assert_eq!(app.world().resource::<Counter>().0, 55);
    connection.execute_batch("COMMIT").unwrap();
    settle(&mut app);

    app.close_request();
    assert!(app.world().resource::<SaveStatus>().handle().is_none());
    assert_eq!(app.world().resource::<Counter>().0, 0);
    assert_eq!(saved_values(&connection, "counter"), [10]);
    assert_eq!(generation(&path), 2);
}

fn edit_dirty(world: &mut World, entity: Entity, value: u64) {
    world.get_mut::<DirtyValue>(entity).unwrap().0 = value;
    world
        .run_system_once(move |mut dirty: Dirty<DirtyValue>| dirty.mark(entity))
        .unwrap();
}

fn saved_values(connection: &Connection, key: &str) -> Vec<u64> {
    connection
        .prepare("SELECT payload FROM records WHERE type_key = ?1 ORDER BY entity")
        .unwrap()
        .query_map([key], |row| row.get::<_, Vec<u8>>(0))
        .unwrap()
        .map(|row| serde_json::from_slice(&row.unwrap()).unwrap())
        .collect()
}
