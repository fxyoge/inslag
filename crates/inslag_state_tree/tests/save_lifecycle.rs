#![cfg(feature = "persistence")]
use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_time::{Real, Time, TimePlugin};
use inslag_persist::{
    Error, LoadOutcome, SaveCompleted, SaveOpened, SavePlugin, SaveRequest, SaveSettings,
    SaveStatus,
};
use inslag_state_tree::persistence::AppPersistenceExt;
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{Duration, Instant},
};
#[path = "../../inslag_persist/tests/support/requests.rs"]
mod requests;
use requests::Requests;

#[derive(Resource, Default, Serialize, Deserialize)]
struct Score(u64);
#[derive(Resource)]
struct Ready(bool);

fn app(directory: &Path) -> App {
    let mut app = App::new();
    app.add_plugins(TimePlugin)
        .insert_resource(Ready(true))
        .init_resource::<Score>()
        .persist_resource::<Score>("score")
        .add_plugins(SavePlugin::sqlite(directory).enabled_when(|ready: Res<Ready>| ready.0))
        .add_systems(SaveOpened, |world: &mut World| {
            if matches!(
                world.resource::<SaveStatus>().load(),
                Some(Ok(LoadOutcome::Fresh))
            ) {
                world.insert_resource(Score(10));
            }
        });
    app
}

fn request(app: &mut App, request: SaveRequest) {
    app.world_mut().write_message(request);
    app.update();
    settle(app);
}

fn settle(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while {
        let status = app.world().resource::<SaveStatus>();
        status.is_loading() || status.is_saving()
    } {
        assert!(Instant::now() < deadline, "persistence did not settle");
        std::thread::yield_now();
        app.update();
    }
}

#[test]
fn never_suppresses_automatic_saves_but_manual_saves_restore_in_a_fresh_app() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = app(directory.path());
    first.new_request();
    first.world_mut().resource_mut::<Score>().0 = 93;
    first
        .world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_secs(3600));
    first.update();
    assert!(first.list_request().unwrap().is_empty());
    request(&mut first, SaveRequest::Save);
    assert!(
        first
            .world()
            .resource::<SaveStatus>()
            .last_save()
            .unwrap()
            .is_ok()
    );
    let mut second = app(directory.path());
    let saves = second.list_request().unwrap();
    second.load_request(saves[0].handle).unwrap();
    assert!(matches!(
        second.world().resource::<SaveStatus>().load(),
        Some(Ok(LoadOutcome::Loaded))
    ));
    assert_eq!(second.world().resource::<Score>().0, 93);
}

#[test]
fn settings_changes_reset_deadlines_and_never_cancels_an_existing_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    let save = app.new_request();
    app.world_mut().resource_mut::<SaveSettings>().interval = Some(Duration::from_secs(10));
    app.update();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_secs(11));
    app.update();
    settle(&mut app);
    let listed = app.list_request().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].handle, save);
    let generation = app
        .world()
        .resource::<SaveStatus>()
        .last_save()
        .unwrap()
        .as_ref()
        .unwrap()
        .generation;
    app.world_mut().resource_mut::<SaveSettings>().interval = None;
    app.update();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_secs(3600));
    app.update();
    assert_eq!(
        app.world()
            .resource::<SaveStatus>()
            .last_save()
            .unwrap()
            .as_ref()
            .unwrap()
            .generation,
        generation
    );
}

#[test]
fn caller_can_discard_a_session_after_a_failed_save() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    let save = app.new_request();
    app.world_mut().resource_mut::<Score>().0 = 21;
    app.save_request().unwrap();
    app.world_mut().resource_mut::<Score>().0 = 42;
    app.world_mut().resource_mut::<Ready>().0 = false;
    assert!(
        app.save_request()
            .is_err_and(|error| matches!(*error, Error::NotReady))
    );
    assert_eq!(app.world().resource::<Score>().0, 42);
    assert!(matches!(app.request(SaveRequest::New),
        SaveCompleted::New(Err(error)) if matches!(*error, Error::AlreadyOpen)));
    app.close_request();
    assert_eq!(app.world().resource::<SaveStatus>().handle(), None);
    let mut restored = self::app(directory.path());
    restored.load_request(save).unwrap();
    assert_eq!(restored.world().resource::<Score>().0, 21);
}

#[test]
fn a_caller_can_save_before_exiting_without_autosave() {
    use bevy_app::AppExit;
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    let save = app.new_request();
    app.world_mut().resource_mut::<Score>().0 = 83;
    request(&mut app, SaveRequest::Save);
    assert_eq!(
        app.world()
            .resource::<SaveStatus>()
            .last_save()
            .unwrap()
            .as_ref()
            .unwrap()
            .generation,
        1
    );
    app.world_mut().write_message(AppExit::Success);
    app.update();
    assert!(app.should_exit().is_some());
    let mut restored = self::app(directory.path());
    restored.load_request(save).unwrap();
    assert_eq!(restored.world().resource::<Score>().0, 83);
}

#[test]
fn preparation_commands_get_a_normal_cleanup_frame_before_capture() {
    use bevy_app::Update;
    use inslag_persist::PrepareSave;
    #[derive(Component, Serialize, Deserialize)]
    struct Owner;
    #[derive(Resource, Default, Serialize, Deserialize)]
    struct Owners(
        #[serde(with = "inslag_state_tree::persistence::entity_map")]
        bevy_ecs::entity::EntityHashMap<()>,
    );
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    app.persist_component::<Owner>("owner")
        .persist_resource::<Owners>("owners")
        .init_resource::<Owners>()
        .add_systems(
            PrepareSave,
            |owners: Query<Entity, With<Owner>>, mut commands: Commands| {
                for entity in &owners {
                    commands.entity(entity).despawn();
                }
            },
        )
        .add_systems(
            Update,
            |mut removed: RemovedComponents<Owner>, mut owners: ResMut<Owners>| {
                for entity in removed.read() {
                    owners.0.remove(&entity);
                }
            },
        );
    app.new_request();
    let owner = app.world_mut().spawn(Owner).id();
    app.world_mut()
        .insert_resource(Owners([(owner, ())].into()));
    app.world_mut().write_message(SaveRequest::Save);
    app.update();
    assert!(
        std::fs::read_dir(directory.path())
            .unwrap()
            .next()
            .is_none()
    );
    assert!(app.world().resource::<SaveStatus>().is_saving());
    app.world_mut().write_message(SaveRequest::Save);
    app.update();
    settle(&mut app);
    assert_eq!(
        app.world()
            .resource::<SaveStatus>()
            .last_save()
            .unwrap()
            .as_ref()
            .unwrap()
            .generation,
        1
    );
    assert!(app.world().resource::<Owners>().0.is_empty());
}
