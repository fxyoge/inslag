mod support;

use bevy_ecs::{prelude::*, system::RunSystemOnce};
use divan::{Bencher, counter::ItemsCount};
use inslag_state_tree::persistence::Checkpoint;
use support::{BenchmarkApp, Executor};

fn main() {
    divan::main();
}

#[divan::bench(args = [1_000, 10_000], sample_count = 10)]
fn capture_datamodel(bencher: Bencher<'_, '_>, instances: usize) {
    let mut benchmark = benchmark(instances);
    bencher.counter(ItemsCount::new(instances)).bench_local(|| {
        divan::black_box(Checkpoint::capture(benchmark.app.world_mut()).unwrap());
    });
}

#[divan::bench(args = [1_000, 10_000], sample_count = 10)]
fn restore_datamodel(bencher: Bencher<'_, '_>, instances: usize) {
    bencher
        .counter(ItemsCount::new(instances))
        .with_inputs(|| {
            let mut benchmark = benchmark(instances);
            let checkpoint = Checkpoint::capture(benchmark.app.world_mut()).unwrap();
            (benchmark, checkpoint)
        })
        .bench_local_values(|(mut benchmark, checkpoint)| {
            checkpoint.restore(benchmark.app.world_mut()).unwrap();
            benchmark.run_schedule();
            divan::black_box(benchmark)
        });
}

fn benchmark(instances: usize) -> BenchmarkApp {
    let mut benchmark = BenchmarkApp::empty(support::activity_definition(), Executor::Parallel);
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
