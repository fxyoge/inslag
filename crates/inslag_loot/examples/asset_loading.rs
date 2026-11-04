use bevy_app::{App, AppExit, ScheduleRunnerPlugin, Startup, TaskPoolPlugin, Update};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use inslag_loot::prelude::*;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, LootOutput)]
struct ItemSpec(String);

#[derive(Component, Debug)]
struct DefinitionId(String);

#[derive(LootArgs)]
struct ItemArgs {
    #[loot(primary)]
    item: String,
}

fn item(args: ItemArgs) -> ItemSpec {
    ItemSpec(args.item)
}

fn request_reward(mut requests: MessageWriter<LootRequest<ItemSpec>>) {
    requests.write(LootRequest::new("reward").seed(7));
}

fn spawn_rewards(
    mut commands: Commands,
    mut finished: MessageReader<LootFinished<ItemSpec>>,
    mut exit: MessageWriter<AppExit>,
) {
    for finished in finished.read() {
        match &finished.result {
            Ok(items) => {
                for item in items {
                    let definition = DefinitionId(item.0.clone());
                    println!("Received {}", definition.0);
                    commands.spawn(definition);
                }
                exit.write(AppExit::Success);
            }
            Err(error) => {
                eprintln!("Reward generation failed: {error}");
                exit.write(AppExit::error());
            }
        }
    }
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            file_path: format!("{}/examples/assets", env!("CARGO_MANIFEST_DIR")),
            ..Default::default()
        },
        LootPlugin::default().with_sources(["asset_rewards.loot"]),
    ))
    .add_loot_function("item", item)
    .add_loot_output::<ItemSpec>()
    .add_systems(Startup, request_reward)
    .add_systems(Update, spawn_rewards);
    app
}

fn main() -> AppExit {
    app()
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(1)))
        .run()
}
