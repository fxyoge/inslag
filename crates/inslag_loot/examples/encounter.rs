use bevy_app::{App, AppExit, ScheduleRunnerPlugin, Startup, TaskPoolPlugin, Update};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use inslag_loot::prelude::*;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, LootOutput)]
struct ActorSpec {
    definition: String,
    role: String,
}

#[derive(Component, Debug)]
struct DefinitionId(String);

#[derive(Component, Debug)]
struct Role(String);

#[derive(LootArgs)]
struct ActorArgs {
    #[loot(primary)]
    actor: String,
    #[loot(default = "member".into())]
    role: String,
}

fn actor(args: ActorArgs) -> ActorSpec {
    ActorSpec {
        definition: args.actor,
        role: args.role,
    }
}

fn request_encounter(mut requests: MessageWriter<LootRequest<ActorSpec>>) {
    requests.write(LootRequest::new("forest-encounter").seed(4));
}

fn spawn_encounter(
    mut commands: Commands,
    mut finished: MessageReader<LootFinished<ActorSpec>>,
    mut exit: MessageWriter<AppExit>,
) {
    for roll in finished.read() {
        match &roll.result {
            Ok(actors) => {
                for actor in actors {
                    let definition = DefinitionId(actor.definition.clone());
                    let role = Role(actor.role.clone());
                    println!("Spawning {} ({})", definition.0, role.0);
                    commands.spawn((definition, role));
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
        LootPlugin::default().with_sources(["encounter.loot"]),
    ))
    .add_loot_function("actor", actor)
    .add_loot_output::<ActorSpec>()
    .add_systems(Startup, request_encounter)
    .add_systems(Update, spawn_encounter);
    app
}
