use bevy_app::{App, Last, TaskPoolPlugin, Update};
use bevy_asset::{AssetPlugin, Assets};
use bevy_ecs::prelude::*;
use bevy_time::TimePlugin;
use inslag_persist::{SavePlugin, SaveRequest, SaveStatus};
#[path = "../../inslag_persist/tests/support/requests.rs"]
mod requests;
use inslag_state_tree::{persistence::AppPersistenceExt, prelude::*};
use requests::Requests;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Component, Serialize, Deserialize)]
struct Sheet {
    ink: Vec<bool>,
    stage: Vec<bool>,
}

fn main() {
    for sheets in [0, 64, 512] {
        let directory = tempfile::tempdir().unwrap();
        let mut app = app(sheets, directory.path());
        app.save_request().unwrap();
        let mut idle = Vec::new();
        let mut saves = Vec::new();
        let mut payload_bytes = 0;
        // Setup and the initial commit are excluded; save frames are reported
        // separately so a rare stall cannot hide in the overall median.
        for frame in 0..1200 {
            let save = frame % 60 == 0;
            if save {
                app.world_mut().write_message(SaveRequest::Save);
            }
            let start = Instant::now();
            app.update();
            if save {
                saves.push(start.elapsed());
                payload_bytes = finish_save(&mut app);
            } else {
                idle.push(start.elapsed());
            }
        }
        println!("sheets={sheets} payload_bytes={payload_bytes}");
        report(sheets, "idle-update", &mut idle);
        report(sheets, "save-update", &mut saves);
    }
}

fn app(sheets: usize, directory: &std::path::Path) -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        TimePlugin,
        AssetPlugin::default(),
        StateTreePlugin::new(Update),
        SavePlugin::sqlite(directory),
    ))
    .persist_component::<Sheet>("profile.sheet");
    app.new_request();
    let mut builder = StateTreeBuilder::new("profile-tree");
    let root = builder.compound_root("root").unwrap();
    let idle = builder.atomic("idle", root).unwrap();
    builder.initial(root, idle).unwrap();
    let definition = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(builder.build().unwrap());
    for _ in 0..32 {
        app.world_mut()
            .spawn((StateTreeInstance::new(definition.clone()), PersistStateTree));
    }
    for _ in 0..sheets {
        app.world_mut().spawn((
            StateTreeInstance::new(definition.clone()),
            PersistStateTree,
            Sheet {
                ink: (0..1600).map(|i| i % 3 == 0).collect(),
                stage: vec![false; 1600],
            },
        ));
    }
    app.update();
    app
}

fn finish_save(app: &mut App) -> usize {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = app.world().resource::<SaveStatus>();
        if !status.is_saving() {
            return status.last_save().unwrap().as_ref().unwrap().bytes;
        }
        assert!(Instant::now() < deadline, "persistence did not settle");
        std::thread::yield_now();
        app.world_mut().run_schedule(Last);
    }
}

fn report(sheets: usize, phase: &str, samples: &mut [Duration]) {
    samples.sort_unstable();
    let ms = |n: usize| samples[(samples.len() - 1) * n / 100].as_secs_f64() * 1000.0;
    println!(
        "sheets={sheets} phase={phase} samples={} p50_ms={:.3} p95_ms={:.3} p99_ms={:.3} max_ms={:.3}",
        samples.len(),
        ms(50),
        ms(95),
        ms(99),
        ms(100),
    );
}
