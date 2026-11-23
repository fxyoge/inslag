mod support;

use bevy_ecs::{prelude::*, system::RunSystemOnce};
use divan::{Bencher, counter::ItemsCount};
use inslag_persist::SavePlugin;
#[path = "../../inslag_persist/tests/support/requests.rs"]
mod requests;
use requests::Requests;
use support::{BenchmarkApp, Executor};

fn main() {
    divan::main();
}

#[divan::bench(args = [1_000, 10_000], sample_count = 10)]
fn save_datamodel(bencher: Bencher<'_, '_>, instances: usize) {
    let directory = tempfile::tempdir().unwrap();
    let mut benchmark = benchmark(instances, &directory);
    benchmark.app.save_request().unwrap();
    bencher.counter(ItemsCount::new(instances)).bench_local(|| {
        divan::black_box(benchmark.app.save_request().unwrap());
    });
}

#[divan::bench(args = [1_000, 10_000], sample_count = 10)]
fn restore_datamodel(bencher: Bencher<'_, '_>, instances: usize) {
    bencher
        .counter(ItemsCount::new(instances))
        .with_inputs(|| {
            let directory = tempfile::tempdir().unwrap();
            let mut benchmark = benchmark(instances, &directory);
            benchmark.app.save_request().unwrap();
            let handle = benchmark
                .app
                .world()
                .resource::<inslag_persist::SaveStatus>()
                .handle()
                .unwrap();
            benchmark.app.close_request();
            (benchmark, directory, handle)
        })
        .bench_local_values(|(mut benchmark, directory, handle)| {
            benchmark.app.load_request(handle).unwrap();
            benchmark.run_schedule();
            divan::black_box((benchmark, directory))
        });
}

fn benchmark(instances: usize, directory: &tempfile::TempDir) -> BenchmarkApp {
    let mut benchmark = BenchmarkApp::empty(support::activity_definition(), Executor::Parallel);
    benchmark
        .app
        .add_plugins(SavePlugin::sqlite(directory.path()));
    benchmark.app.new_request();
    benchmark.register_benchmark_activity();
    benchmark.spawn_instances(instances);
    benchmark
        .app
        .world_mut()
        .run_system_once(
            |mut commands: Commands,
             trees: Query<Entity, With<inslag_state_tree::StateTreeInstance>>| {
                for entity in &trees {
                    commands
                        .entity(entity)
                        .insert(inslag_state_tree::PersistStateTree);
                }
            },
        )
        .unwrap();
    benchmark.run_schedule();
    benchmark
}
