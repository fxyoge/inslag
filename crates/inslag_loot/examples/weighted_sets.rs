use bevy_app::{App, AppExit, ScheduleRunnerPlugin, Startup, TaskPoolPlugin, Update};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use inslag_loot::prelude::*;
use std::time::Duration;

#[derive(Component, Debug)]
struct DefinitionId(String);

fn request_encounter(mut requests: MessageWriter<LootRequest<String>>) {
    requests.write(
        LootRequest::new("forest")
            .input("rare_weight", 3u32)
            .seed(9),
    );
}

fn spawn_encounter(
    mut commands: Commands,
    mut finished: MessageReader<LootFinished<String>>,
    mut exit: MessageWriter<AppExit>,
) {
    for roll in finished.read() {
        match &roll.result {
            Ok(definitions) => {
                for definition in definitions {
                    let definition = DefinitionId(definition.clone());
                    println!("Spawning {}", definition.0);
                    commands.spawn(definition);
                }
                exit.write(AppExit::Success);
            }
            Err(error) => {
                eprintln!("{error}");
                exit.write(AppExit::error());
            }
        }
    }
}

fn main() -> AppExit {
    app()
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(1)))
        .run()
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            file_path: format!("{}/examples/assets", env!("CARGO_MANIFEST_DIR")),
            ..Default::default()
        },
        LootPlugin::default().with_sources(["weighted_sets.loot", "compositions.loot"]),
    ))
    .add_loot_output::<String>()
    .add_systems(Startup, request_encounter)
    .add_systems(Update, spawn_encounter);
    app
}
