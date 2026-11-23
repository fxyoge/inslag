//! cargo run -p inslag_persist --example headless -- /tmp/opencode/inslag-persist-example
use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use bevy_time::TimePlugin;
use inslag_persist::{
    AppSaveExt, Capture, LoadOutcome, SaveOpened, SavePlugin, SaveRequest, SaveStatus, component,
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Component, Serialize, Deserialize)]
struct Node {
    ticks: u64,
    #[entities]
    #[serde(with = "inslag_persist::entity")]
    peer: Entity,
}

#[derive(Resource, Default, Serialize, Deserialize)]
struct Account {
    credits: u64,
}

#[derive(Component)]
struct Transient;

fn fresh(status: Res<SaveStatus>) -> bool {
    matches!(status.load(), Some(Ok(LoadOutcome::Fresh)))
}

fn initialize(mut commands: Commands) {
    let a = commands.spawn_empty().id();
    let b = commands.spawn_empty().id();
    commands.entity(a).insert(Node { ticks: 0, peer: b });
    commands.entity(b).insert(Node { ticks: 0, peer: a });
}

fn simulate(mut nodes: Query<&mut Node>) {
    for mut node in &mut nodes {
        node.ticks += 1;
    }
}

fn main() {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "saves".into());
    let mut app = App::new();
    app.add_plugins(TimePlugin);
    let plugin = SavePlugin::sqlite(&path).autosave(Duration::from_millis(250));
    app.add_plugins(plugin)
        .persist_resource::<Account>("account")
        .add_systems(
            SaveOpened,
            (|mut commands: Commands| {
                commands.insert_resource(Account { credits: 10 });
            })
            .run_if(fresh),
        );
    app.persist(component::<Node>("node").capture(Capture::Changed))
        .add_systems(SaveOpened, initialize.run_if(fresh))
        .add_systems(Update, simulate);
    app.world_mut().spawn(Transient);
    app.world_mut().write_message(SaveRequest::New);
    app.update();
    settle(&mut app);
    let status = app.world().resource::<SaveStatus>();
    println!("{:?}: {:?}", status.handle(), status.load());
    if !status.can_save() {
        std::process::exit(1);
    }
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(10));
        app.update();
    }
    settle(&mut app);
    app.world_mut().write_message(SaveRequest::Save);
    app.update();
    settle(&mut app);
    let stats = app
        .world()
        .resource::<SaveStatus>()
        .last_save()
        .unwrap()
        .as_ref()
        .unwrap();
    println!("Final save: {stats:?}");
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
