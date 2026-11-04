use bevy_app::{App, TaskPoolPlugin};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use inslag_loot::prelude::*;
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

thread_local! {
    static APP: RefCell<App> = RefCell::new(app());
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            file_path: format!("{}/benches/assets", env!("CARGO_MANIFEST_DIR")),
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
