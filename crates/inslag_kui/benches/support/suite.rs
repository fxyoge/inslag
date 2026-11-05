// Identical workloads in both binaries. Allocation changes ONLY the executor
// and allocator; use CLI sample-count overrides for longer execution passes.

#[divan::bench(args = [1, 10, 100, 1_000, 5_000], sample_count = 10, sample_size = 1)]
fn settled_views(bencher: Bencher<'_, '_>, views: usize) {
    measure(bencher, Config::protocol(views, 1, 10, Work::Idle));
}

#[divan::bench(args = [1, 10, 100, 1_000], sample_count = 10, sample_size = 1)]
fn settled_controllers(bencher: Bencher<'_, '_>, controllers: usize) {
    measure(
        bencher,
        Config::protocol(1_000, controllers, 10, Work::Idle),
    );
}

#[divan::bench(args = [1, 10, 100, 1_000, 5_000], sample_count = 10, sample_size = 1)]
fn settled_independent_views(bencher: Bencher<'_, '_>, views: usize) {
    measure(bencher, Config::protocol(views, views, 10, Work::Idle));
}

#[divan::bench(args = [10, 1_000, 10_000, 25_000], sample_count = 10, sample_size = 1)]
fn settled_nodes(bencher: Bencher<'_, '_>, nodes: usize) {
    measure(bencher, Config::protocol(1, 1, nodes, Work::Idle));
}

#[divan::bench(args = [1, 10, 100, 1_000, 5_000], sample_count = 10, sample_size = 1)]
fn scoped_next(bencher: Bencher<'_, '_>, views: usize) {
    measure(bencher, Config::protocol(views, 1, 10, Work::Next));
}

#[divan::bench(args = [1, 10, 100, 1_000], sample_count = 10, sample_size = 1)]
fn scoped_next_controllers(bencher: Bencher<'_, '_>, controllers: usize) {
    measure(
        bencher,
        Config::protocol(1_000, controllers, 10, Work::Next),
    );
}

#[divan::bench(args = [1, 10, 100, 1_000, 5_000], sample_count = 10, sample_size = 1)]
fn scoped_next_independent(bencher: Bencher<'_, '_>, views: usize) {
    measure(bencher, Config::protocol(views, views, 10, Work::Next));
}

#[divan::bench(args = [1, 1_000, 5_000], sample_count = 10, sample_size = 1)]
fn scoped_activate_independent(bencher: Bencher<'_, '_>, views: usize) {
    measure(bencher, Config::protocol(views, views, 10, Work::Activate));
}

#[divan::bench(args = [1, 100, 1_000, 5_000], sample_count = 10, sample_size = 1)]
fn scoped_previous(bencher: Bencher<'_, '_>, views: usize) {
    measure(bencher, Config::protocol(views, 1, 10, Work::Previous));
}

#[divan::bench(args = [1, 100, 1_000, 5_000], sample_count = 10, sample_size = 1)]
fn scoped_activate(bencher: Bencher<'_, '_>, views: usize) {
    measure(bencher, Config::protocol(views, 1, 10, Work::Activate));
}

#[divan::bench(args = [10, 1_000, 10_000, 25_000], sample_count = 10, sample_size = 1)]
fn huge_scope_next(bencher: Bencher<'_, '_>, nodes: usize) {
    measure(bencher, Config::protocol(1, 1, nodes, Work::Next));
}

#[divan::bench(args = [10, 1_000, 10_000], sample_count = 10, sample_size = 1)]
fn huge_scope_previous(bencher: Bencher<'_, '_>, nodes: usize) {
    measure(bencher, Config::protocol(1, 1, nodes, Work::Previous));
}

#[divan::bench(args = [10, 1_000, 10_000], sample_count = 10, sample_size = 1)]
fn huge_scope_activate(bencher: Bencher<'_, '_>, nodes: usize) {
    measure(bencher, Config::protocol(1, 1, nodes, Work::Activate));
}

#[divan::bench(args = [10, 1_000, 10_000], sample_count = 10, sample_size = 1)]
fn mixed_availability(bencher: Bencher<'_, '_>, nodes: usize) {
    let mut config = Config::protocol(1, 1, nodes, Work::Next);
    config.scene = Scene::MixedFocus;
    measure(bencher, config);
}

#[divan::bench(args = [1, 8, 64, 256], sample_count = 10, sample_size = 1)]
fn input_burst(bencher: Bencher<'_, '_>, count: usize) {
    let mut config = Config::protocol(1_000, 1, 10, Work::Burst);
    config.burst = count;
    measure(bencher, config);
}

#[divan::bench(args = [1, 10, 100, 1_000], sample_count = 5, sample_size = 1)]
fn mount_ready_reentry(bencher: Bencher<'_, '_>, views: usize) {
    measure(bencher, Config::protocol(views, 1, 10, Work::Restart));
}

#[divan::bench(args = [1, 10, 100], sample_count = 5, sample_size = 1)]
fn cancellation_reentry(bencher: Bencher<'_, '_>, views: usize) {
    measure(bencher, Config::protocol(views, 1, 10, Work::CancelReenter));
}

#[divan::bench(args = [1, 100, 1_000], sample_count = 5, sample_size = 1)]
fn cancelled_generation_input(bencher: Bencher<'_, '_>, views: usize) {
    measure(
        bencher,
        Config::protocol(views, 1, 10, Work::StaleGeneration),
    );
}

// Reload only target.kui (one view/controller), leaving unrelated mounts settled.
// This deliberately includes the public memory-reader load and completion loop.
#[divan::bench(args = [1, 100, 1_000], sample_count = 5, sample_size = 1)]
fn sparse_reload_ready(bencher: Bencher<'_, '_>, views: usize) {
    measure(bencher, Config::protocol(views, 1, 10, Work::Reload));
}

#[divan::bench(args = [1, 100, 1_000, 5_000], sample_count = 5, sample_size = 1)]
fn sparse_mount_ready(bencher: Bencher<'_, '_>, views: usize) {
    measure(bencher, Config::protocol(views, 1, 10, Work::RestartTarget));
}

#[divan::bench(types = [Native, Scxml], args = [10, 1_000, 10_000, 100_000], sample_count = 10, sample_size = 1)]
fn idle_static<B: Choice>(bencher: Bencher<'_, '_>, nodes: usize) {
    measure(bencher, Config::new(B::BACKEND, Scene::Static, nodes));
}

#[divan::bench(types = [Native, Scxml], args = [10, 1_000, 10_000], sample_count = 10, sample_size = 1)]
fn idle_scalar<B: Choice>(bencher: Bencher<'_, '_>, nodes: usize) {
    measure(bencher, Config::new(B::BACKEND, Scene::Scalar, nodes));
}

#[divan::bench(types = [Native, Scxml], args = [10, 1_000, 10_000], sample_count = 10, sample_size = 1)]
fn dense_fanout<B: Choice>(bencher: Bencher<'_, '_>, nodes: usize) {
    let mut config = Config::new(B::BACKEND, Scene::Scalar, nodes);
    config.work = Work::Toggle;
    measure(bencher, config);
}

#[divan::bench(types = [Native, Scxml], args = [1_000, 10_000], sample_count = 10, sample_size = 1)]
fn sparse_same_view<B: Choice>(bencher: Bencher<'_, '_>, nodes: usize) {
    let mut config = Config::new(B::BACKEND, Scene::Sparse, nodes);
    config.work = Work::Sparse;
    measure(bencher, config);
}

#[divan::bench(types = [Native, Scxml], args = [10, 1_000, 5_000], sample_count = 10, sample_size = 1)]
fn sparse_independent<B: Choice>(bencher: Bencher<'_, '_>, views: usize) {
    let mut config = Config::new(B::BACKEND, Scene::Sparse, 1);
    config.views = views;
    config.controllers = views;
    config.work = Work::Sparse;
    measure(bencher, config);
}

#[divan::bench(types = [Native, Scxml], args = [1_000, 10_000], sample_count = 10, sample_size = 1)]
fn dense_expression<B: Choice>(bencher: Bencher<'_, '_>, nodes: usize) {
    let mut config = Config::new(B::BACKEND, Scene::Expression, nodes);
    config.work = Work::Toggle;
    measure(bencher, config);
}

#[divan::bench(types = [Native, Scxml], args = [100, 1_000, 10_000], sample_count = 10, sample_size = 1)]
fn active_components<B: Choice>(bencher: Bencher<'_, '_>, nodes: usize) {
    let mut config = Config::new(B::BACKEND, Scene::Component, nodes);
    config.work = Work::Toggle;
    measure(bencher, config);
}

#[divan::bench(types = [Native, Scxml], args = [0, 64, 1_024, 4_096], sample_count = 10, sample_size = 1)]
fn active_shared_root<B: Choice>(bencher: Bencher<'_, '_>, payload: usize) {
    let mut config = Config::new(B::BACKEND, Scene::Shared, 128);
    config.payload = payload;
    config.work = Work::Toggle;
    measure(bencher, config);
}

#[divan::bench(args = [0, 64, 1_024, 4_096], sample_count = 10, sample_size = 1)]
fn change_only_unread_domain(bencher: Bencher<'_, '_>, payload: usize) {
    let mut config = Config::new(Backend::Scxml, Scene::Shared, 128);
    config.payload = payload;
    config.work = Work::Unrelated;
    measure(bencher, config);
}

#[divan::bench(args = [0, 64, 1_024, 4_096], sample_count = 10, sample_size = 1)]
fn idle_shared_root(bencher: Bencher<'_, '_>, payload: usize) {
    let mut config = Config::new(Backend::Scxml, Scene::Shared, 128);
    config.payload = payload;
    measure(bencher, config);
}

#[divan::bench(args = [100, 1_000, 10_000], sample_count = 10, sample_size = 1)]
fn idle_components(bencher: Bencher<'_, '_>, nodes: usize) {
    measure(
        bencher,
        Config::new(Backend::Scxml, Scene::Component, nodes),
    );
}

#[divan::bench(args = [1_000, 10_000], sample_count = 5, sample_size = 1)]
fn mount_components(bencher: Bencher<'_, '_>, nodes: usize) {
    let mut config = Config::new(Backend::Scxml, Scene::Component, nodes);
    config.work = Work::Restart;
    measure(bencher, config);
}

#[divan::bench(args = [1_000, 10_000], sample_count = 5, sample_size = 1)]
fn reload_components(bencher: Bencher<'_, '_>, nodes: usize) {
    let mut config = Config::new(Backend::Scxml, Scene::Component, nodes);
    config.work = Work::Reload;
    measure(bencher, config);
}

#[divan::bench(types = [Native, Scxml], args = [100, 1_000, 10_000], sample_count = 10, sample_size = 1)]
fn idle_repeat<B: Choice>(bencher: Bencher<'_, '_>, elements: usize) {
    measure(bencher, repeat_config(B::BACKEND, elements, Work::Idle));
}

#[divan::bench(types = [Native, Scxml], args = [100, 1_000, 10_000], sample_count = 10, sample_size = 1)]
fn sparse_repeat<B: Choice>(bencher: Bencher<'_, '_>, elements: usize) {
    measure(bencher, repeat_config(B::BACKEND, elements, Work::Sparse));
}

#[divan::bench(args = [100, 1_000, 10_000], sample_count = 10, sample_size = 1)]
fn reorder_repeat(bencher: Bencher<'_, '_>, elements: usize) {
    measure(
        bencher,
        repeat_config(Backend::Scxml, elements, Work::Reorder),
    );
}

#[divan::bench(args = [100, 1_000, 10_000], sample_count = 10, sample_size = 1)]
fn churn_repeat(bencher: Bencher<'_, '_>, elements: usize) {
    measure(
        bencher,
        repeat_config(Backend::Scxml, elements, Work::Churn),
    );
}

#[divan::bench(types = [Native, Scxml], args = [1_000, 10_000], sample_count = 10, sample_size = 1)]
fn repeat_external<B: Choice>(bencher: Bencher<'_, '_>, elements: usize) {
    let mut config = repeat_config(B::BACKEND, elements, Work::Toggle);
    config.scene = Scene::RepeatExternal;
    measure(bencher, config);
}

#[divan::bench(args = [1_000, 10_000], sample_count = 5, sample_size = 1)]
fn mount_repeat(bencher: Bencher<'_, '_>, elements: usize) {
    measure(
        bencher,
        repeat_config(Backend::Scxml, elements, Work::Restart),
    );
}

#[divan::bench(types = [Native, Flair, Scxml], args = [1_000, 10_000], sample_count = 10, sample_size = 1)]
fn cpu_idle_scalar<B: Choice>(bencher: Bencher<'_, '_>, nodes: usize) {
    let mut config = Config::new(B::BACKEND, Scene::Scalar, nodes);
    config.pipeline = Pipeline::Cpu;
    measure(bencher, config);
}

#[divan::bench(types = [Native, Flair, Scxml], args = [1_000, 10_000], sample_count = 10, sample_size = 1)]
fn cpu_dense_text<B: Choice>(bencher: Bencher<'_, '_>, nodes: usize) {
    let mut config = Config::new(B::BACKEND, Scene::Text, nodes);
    config.pipeline = Pipeline::Cpu;
    config.work = Work::Toggle;
    measure(bencher, config);
}

fn measure(bencher: Bencher<'_, '_>, config: Config) {
    let mut fixture = Fixture::new(config);
    if SINGLE_THREADED {
        fixture.single_threaded();
    }
    bencher.bench_local(|| fixture.step());
    fixture.verify();
}

fn repeat_config(backend: Backend, elements: usize, work: Work) -> Config {
    let mut config = Config::new(backend, Scene::Repeat, elements.min(1_000));
    config.views = elements.div_ceil(1_000);
    config.controllers = config.views;
    config.work = work;
    config
}
