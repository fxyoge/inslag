mod support;

use divan::{AllocProfiler, Bencher};
use support::{Backend, Choice, Config, Fixture, Flair, Native, Pipeline, Scene, Scxml, Work};

const SINGLE_THREADED: bool = true;

#[global_allocator]
static ALLOC: AllocProfiler = AllocProfiler::system();

fn main() {
    // SingleThreadedExecutor does NOT serialize Query::par_iter_mut(). Its
    // explicit compute-pool workers would be invisible to Divan's caller-thread
    // profiler. Initialize this global pool before any plugin can create it.
    bevy::tasks::ComputeTaskPool::get_or_init(|| {
        bevy::tasks::TaskPoolBuilder::new().num_threads(0).build()
    });
    assert_eq!(bevy::tasks::ComputeTaskPool::get().thread_num(), 0);
    divan::main();
}

include!("support/suite.rs");
