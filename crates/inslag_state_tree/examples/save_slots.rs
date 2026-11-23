//! Annotated data and game-specific fresh-world population.
//! `cargo run --features persistence --example save_slots -- /tmp/saves`
#![cfg(feature = "persistence")]
use bevy_app::{App, TaskPoolPlugin, Update};
use bevy_asset::{AssetPlugin, Assets, Handle};
use bevy_ecs::prelude::*;
use bevy_time::TimePlugin;
use inslag_persist::{LoadOutcome, SaveCompleted, SaveOpened, SavePlugin, SaveRequest, SaveStatus};
use inslag_state_tree::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[inslag_persist::save(resource, key = "example.score")]
#[derive(Resource, Default, Serialize, Deserialize)]
struct Score(u64);
#[derive(Resource)]
struct Definition(Handle<StateTreeDefinition>);

fn fresh(status: Res<SaveStatus>, definition: Res<Definition>, mut commands: Commands) {
    if matches!(status.load(), Some(Ok(LoadOutcome::Fresh))) {
        commands.insert_resource(Score(10));
        commands.spawn((
            StateTreeInstance::new(definition.0.clone()),
            PersistStateTree,
        ));
    }
}

fn main() {
    let directory = std::env::args_os().nth(1).expect("pass a save directory");
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        AssetPlugin::default(),
        StateTreePlugin::new(Update),
    ))
    .add_plugins(SavePlugin::sqlite(directory))
    .add_systems(SaveOpened, fresh);
    let mut builder = StateTreeBuilder::new("example");
    let root = builder.compound_root("root").unwrap();
    let idle = builder.atomic("idle", root).unwrap();
    builder.initial(root, idle).unwrap();
    let definition = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(builder.build().unwrap());
    app.insert_resource(Definition(definition));
    let SaveCompleted::List(saves) = request(&mut app, SaveRequest::List) else {
        unreachable!()
    };
    let saves = saves.unwrap();
    let open = saves
        .iter()
        .max_by_key(|save| save.created_at)
        .map_or(SaveRequest::New, |save| SaveRequest::Load(save.handle));
    app.world_mut().write_message(open);
    app.update();
    settle(&mut app);
    assert!(app.world().resource::<SaveStatus>().can_save());
    app.world_mut().resource_mut::<Score>().0 += 1;
    app.world_mut().write_message(SaveRequest::Save);
    app.update();
    settle(&mut app);
    assert!(
        app.world()
            .resource::<SaveStatus>()
            .last_save()
            .unwrap()
            .is_ok()
    );
    println!("saved score {}", app.world().resource::<Score>().0);
}

fn request(app: &mut App, request: SaveRequest) -> SaveCompleted {
    let mut cursor = app
        .world()
        .resource::<Messages<SaveCompleted>>()
        .get_cursor_current();
    app.world_mut().write_message(request);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.update();
        if let Some(completion) = cursor.read(app.world().resource()).next() {
            return completion.clone();
        }
        assert!(Instant::now() < deadline, "persistence did not settle");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn settle(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while {
        let status = app.world().resource::<SaveStatus>();
        status.is_loading() || status.is_saving()
    } {
        assert!(Instant::now() < deadline, "persistence did not settle");
        std::thread::sleep(Duration::from_millis(1));
        app.update();
    }
}
