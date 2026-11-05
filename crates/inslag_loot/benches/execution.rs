use bevy_app::{App, TaskPoolPlugin};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use inslag_loot::prelude::*;
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

thread_local! {
    static APP: RefCell<App> = RefCell::new(app(format!("{}/benches/assets", env!("CARGO_MANIFEST_DIR"))));
    static BATCH_APP: RefCell<(App, tempfile::TempDir)> = RefCell::new(batch_app());
}

fn batch_app() -> (App, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let mut source = include_str!("assets/execution.loot").to_string();
    source.push_str("table \"literal\"\n  1\n");
    for count in [64, 1024] {
        source.push_str(&format!("table \"weighted{count}\"\n  choose\n"));
        for index in 0..count {
            source.push_str(&format!("    {index} weight={}\n", index % 7 + 1));
        }
    }
    source.push_str("table \"graph\"\n  choose\n");
    for index in 0..64 {
        source.push_str(&format!("    #branch{index}\n"));
    }
    for index in 0..64 {
        source.push_str(&format!("table \"branch{index}\"\n"));
        for value in 0..16 {
            source.push_str(&format!("  {value}\n"));
        }
    }
    std::fs::write(directory.path().join("execution.loot"), source).unwrap();
    let app = app(directory.path().to_string_lossy().into_owned());
    (app, directory)
}

fn app(file_path: String) -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            file_path,
            ..Default::default()
        },
        LootPlugin::default().with_sources(["execution.loot"]),
    ))
    .add_loot_output::<i64>();
    let mut cursor = app
        .world()
        .resource::<Messages<LootFinished<i64>>>()
        .get_cursor_current();
    app.world_mut()
        .write_message(LootRequest::<i64>::new("root").seed(7));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "benchmark source did not load");
        app.update();
        if let Some(finished) = cursor
            .read(app.world().resource::<Messages<LootFinished<i64>>>())
            .next()
        {
            finished.result.as_ref().unwrap();
            return app;
        }
        std::thread::yield_now();
    }
}

fn main() {
    divan::main();
}

#[divan::bench]
fn weighted_composition() {
    APP.with_borrow_mut(|app| {
        let mut cursor = app
            .world()
            .resource::<Messages<LootFinished<i64>>>()
            .get_cursor_current();
        app.world_mut()
            .write_message(LootRequest::<i64>::new("root").seed(divan::black_box(7)));
        app.update();
        let finished = cursor
            .read(app.world().resource::<Messages<LootFinished<i64>>>())
            .next()
            .unwrap();
        divan::black_box(finished.result.as_ref().unwrap());
    });
}

const BATCH_SIZE: usize = 1024;

#[divan::bench(args = ["literal", "root", "weighted64", "weighted1024", "graph"])]
fn batched_requests(bencher: divan::Bencher, table: &str) {
    BATCH_APP.with_borrow_mut(|(app, _)| {
        roll_batch(app, table);
        bencher
            .counter(divan::counter::ItemsCount::new(BATCH_SIZE))
            .bench_local(|| roll_batch(app, table));
    });
}

fn roll_batch(app: &mut App, table: &str) {
    let mut cursor = app
        .world()
        .resource::<Messages<LootFinished<i64>>>()
        .get_cursor_current();
    for seed in 0..BATCH_SIZE {
        app.world_mut()
            .write_message(LootRequest::<i64>::new(table).seed(seed as u64));
    }
    app.update();
    let mut count = 0;
    for finished in cursor.read(app.world().resource::<Messages<LootFinished<i64>>>()) {
        divan::black_box(finished.result.as_ref().unwrap());
        count += 1;
    }
    assert_eq!(count, BATCH_SIZE);
}
