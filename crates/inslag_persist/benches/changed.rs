use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use divan::{Bencher, black_box};
use inslag_persist::{AppSaveExt, Capture, SavePlugin, component};
#[path = "../tests/support/requests.rs"]
mod requests;
use requests::Requests;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

#[derive(Component, Serialize, Deserialize)]
struct Machine {
    ticks: u64,
    inventory: [u64; 8],
}

#[derive(Resource)]
struct Edits(usize);

fn simulate(mut machines: Query<&mut Machine>, edits: Res<Edits>) {
    for mut machine in machines.iter_mut().take(edits.0) {
        machine.ticks += 1;
    }
}

struct Case {
    app: App,
    _directory: TempDir,
}

#[divan::bench(consts = [1_000, 100_000], args = [0, 1, 100], sample_size = 1, sample_count = 20)]
fn commit<const N: usize>(bencher: Bencher, percent: usize) {
    bencher
        .with_inputs(|| setup(N, percent))
        .bench_local_refs(|case| {
            case.app.update();
            black_box(case.app.save_request().unwrap())
        });
}

// 120 updates and 12 commits per sample.
#[divan::bench(args = [0, 1, 100], sample_size = 1, sample_count = 10)]
fn sustained(bencher: Bencher, percent: usize) {
    bencher
        .with_inputs(|| setup(100_000, percent))
        .bench_local_refs(|case| {
            for tick in 1..=120 {
                case.app.update();
                if tick % 10 == 0 {
                    black_box(case.app.save_request().unwrap());
                }
            }
        });
}

fn setup(entities: usize, percent: usize) -> Case {
    let directory = tempfile::tempdir().unwrap();
    let mut app = App::new();
    app.add_plugins(SavePlugin::sqlite(directory.path()))
        .persist(component::<Machine>("machine").capture(Capture::Changed))
        .insert_resource(Edits(entities * percent / 100))
        .add_systems(Update, simulate);
    app.update();
    app.new_request();
    app.world_mut()
        .spawn_batch((0..entities).map(|index| Machine {
            ticks: 0,
            inventory: [index as u64; 8],
        }));
    app.update();
    assert_eq!(app.save_request().unwrap().upserts, entities);
    Case {
        app,
        _directory: directory,
    }
}

fn main() {
    divan::main();
}
