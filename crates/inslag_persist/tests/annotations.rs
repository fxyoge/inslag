use bevy_app::App;
use bevy_ecs::prelude::*;
use bevy_transform::components::Transform;
use inslag_persist::{Error, SavePlugin, SaveRequest, SaveStatus, save};
use serde::{Deserialize, Serialize};
mod support;
use support::settle;
#[path = "support/requests.rs"]
mod requests;
use requests::{Requests, only_file};
use std::path::Path;

#[save(resource, key = "tiny.score")]
#[derive(Resource, Default, Serialize, Deserialize)]
struct Score(u64);

#[derive(Resource)]
struct Platform(u64, usize);

#[save(resource, key = "tiny.session")]
#[derive(Resource, Serialize, Deserialize)]
struct SessionSeed(u64);
impl FromWorld for SessionSeed {
    fn from_world(world: &mut World) -> Self {
        let mut platform = world.resource_mut::<Platform>();
        platform.1 += 1;
        Self(platform.0)
    }
}

#[save(component, key = "tiny.actor")]
#[derive(Component, Serialize, Deserialize)]
struct Actor {
    #[serde(with = "inslag_persist::entity")]
    peer: Entity,
}

fn app(directory: &Path) -> App {
    let mut app = App::new();
    app.insert_resource(Platform(17, 0))
        .add_plugins(SavePlugin::sqlite(directory));
    app.update();
    app
}
fn request(app: &mut App, request: SaveRequest) {
    app.world_mut().write_message(request);
    app.update();
    settle(app);
}

#[test]
fn annotated_game_needs_only_the_plugin_for_fresh_restore_switch_and_close() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = app(directory.path());
    assert_eq!(first.world().resource::<SessionSeed>().0, 17);
    let a = first.new_request();
    first.world_mut().resource_mut::<Score>().0 = 42;
    let actor = first
        .world_mut()
        .spawn((Name::new("actor"), Transform::from_xyz(1., 2., 3.)))
        .id();
    first
        .world_mut()
        .entity_mut(actor)
        .insert(Actor { peer: actor });
    let camera = first
        .world_mut()
        .spawn((Name::new("camera"), Transform::default()))
        .id();
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
    second.load_request(a).unwrap();
    assert_eq!(second.world().resource::<Score>().0, 42);
    let (entity, restored_actor, name, transform) = second
        .world_mut()
        .query::<(Entity, &Actor, &Name, &Transform)>()
        .single(second.world())
        .unwrap();
    assert_eq!(entity, restored_actor.peer);
    assert_eq!(name.as_str(), "actor");
    assert_eq!(transform.translation.x, 1.);
    first.close_request();
    first.new_request();
    assert_eq!(first.world().resource::<Score>().0, 0);
    assert!(first.world().get_entity(actor).is_err());
    assert!(first.world().get_entity(camera).is_ok());
    first.world_mut().resource_mut::<Score>().0 = 99;
    request(&mut first, SaveRequest::Close);
    assert_eq!(first.world().resource::<Score>().0, 0);
    assert_eq!(first.world().resource::<SessionSeed>().0, 17);
    first.load_request(a).unwrap();
    assert_eq!(first.world().resource::<Score>().0, 42);
}

#[test]
fn failed_restore_leaves_resources_and_entities_intact_without_running_constructors() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = app(directory.path());
    let handle = writer.new_request();
    writer.save_request().unwrap();
    let mut app = app(directory.path());
    app.world_mut().resource_mut::<Score>().0 = 29;
    app.world_mut().resource_mut::<SessionSeed>().0 = 123;
    let actor = app.world_mut().spawn_empty().id();
    app.world_mut()
        .entity_mut(actor)
        .insert(Actor { peer: actor });
    let count = app.world().entities().len();
    let constructors = app.world().resource::<Platform>().1;
    rusqlite::Connection::open(only_file(directory.path()))
        .unwrap()
        .execute(
            "UPDATE records SET payload = ?1 WHERE type_key = 'tiny.score'",
            [b"invalid".as_slice()],
        )
        .unwrap();
    assert!(app.load_request(handle).is_err_and(|error| matches!(
        *error,
        Error::Decode {
            key: "tiny.score",
            ..
        }
    )));
    assert_eq!(app.world().resource::<Score>().0, 29);
    assert_eq!(app.world().resource::<SessionSeed>().0, 123);
    assert!(app.world().get::<Actor>(actor).is_some());
    assert_eq!(app.world().entities().len(), count);
    assert_eq!(app.world().resource::<Platform>().1, constructors);
}

#[test]
fn finishing_a_deferred_open_never_creates_an_unscheduled_save() {
    #[derive(Resource)]
    struct AssetsReady(bool);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saves");
    let mut app = App::new();
    app.insert_resource(Platform(17, 0))
        .insert_resource(AssetsReady(false))
        .add_plugins(SavePlugin::sqlite(&path).enabled_when(|ready: Res<AssetsReady>| ready.0));
    app.world_mut().write_message(SaveRequest::New);
    app.update();
    assert!(app.world().resource::<SaveStatus>().is_loading());
    app.world_mut().resource_mut::<AssetsReady>().0 = true;
    app.update();
    settle(&mut app);
    assert!(app.world().resource::<SaveStatus>().can_save());
    assert!(app.world().resource::<SaveStatus>().last_save().is_none());
    assert!(!path.exists());
}

#[test]
fn new_initializes_fresh_resources() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("saves");
    let mut app = App::new();
    app.insert_resource(Platform(17, 0))
        .add_plugins(SavePlugin::sqlite(&path))
        .insert_resource(Score(99));
    app.new_request();
    assert_eq!(app.world().resource::<Score>().0, 0);
    assert_eq!(app.world().resource::<SessionSeed>().0, 17);
    assert_eq!(app.world().resource::<Platform>().1, 1);
    assert!(!path.exists());
}
