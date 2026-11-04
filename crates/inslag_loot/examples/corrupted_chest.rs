use bevy_app::{App, AppExit, ScheduleRunnerPlugin, Startup, TaskPoolPlugin, Update};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use inslag_loot::prelude::*;
use std::{sync::Arc, time::Duration};

#[derive(Clone, Debug, PartialEq, LootOutput)]
struct ItemSpec {
    definition: String,
    sockets: u32,
}

#[derive(Clone, Debug, PartialEq, LootOutput)]
#[loot(opaque)]
enum Corruption {
    Mark,
    Reforge { power: i64 },
    WhiteSockets(u32),
    Implicit { modifier: String, value: i64 },
}

#[derive(Clone, Debug, PartialEq, LootOutput)]
enum ChestPart {
    Item(ItemSpec),
    Corruption(Corruption),
}

#[derive(Component, Debug)]
struct DefinitionId(String);

#[derive(Component, Debug)]
struct Sockets(u32);

#[derive(Component, Debug)]
struct Corrupted;

#[derive(Component, Debug)]
struct ReforgePower(i64);

#[derive(Component, Debug)]
struct WhiteSockets(u32);

#[derive(Component, Debug)]
struct Implicit {
    modifier: String,
    value: i64,
}

#[derive(LootArgs)]
struct ItemArgs {
    #[loot(primary)]
    item: String,
    sockets: u32,
}

#[derive(LootArgs)]
struct WhitenSockets {
    item: Arc<ItemSpec>,
    #[loot(primary)]
    amount: u32,
}

#[derive(LootArgs)]
struct ImplicitArgs {
    #[loot(primary)]
    modifier: String,
    value: i64,
}

fn item(args: ItemArgs) -> ItemSpec {
    ItemSpec {
        definition: args.item,
        sockets: args.sockets,
    }
}

fn reforge(ctx: &mut LootFunctionContext) -> Corruption {
    Corruption::Reforge {
        power: ctx.rng().i64(1..=20),
    }
}

fn whiten_sockets(args: WhitenSockets) -> Result<Corruption, String> {
    if args.amount > args.item.sockets {
        return Err("not enough sockets".into());
    }
    Ok(Corruption::WhiteSockets(args.amount))
}

fn implicit(args: ImplicitArgs) -> Corruption {
    Corruption::Implicit {
        modifier: args.modifier,
        value: args.value,
    }
}

fn request_chest(mut requests: MessageWriter<LootRequest<ChestPart>>) {
    requests.write(LootRequest::new("corrupted-chest").seed(7));
}

fn open_chest(
    mut commands: Commands,
    mut finished: MessageReader<LootFinished<ChestPart>>,
    mut exit: MessageWriter<AppExit>,
) {
    for roll in finished.read() {
        let generated = match &roll.result {
            Ok(generated) => generated,
            Err(error) => {
                eprintln!("{error}");
                exit.write(AppExit::error());
                continue;
            }
        };

        let mut entity = commands.spawn_empty();
        for part in generated.iter().cloned() {
            match part {
                ChestPart::Item(item) => {
                    let definition = DefinitionId(item.definition);
                    let sockets = Sockets(item.sockets);
                    println!("Received {} with {} sockets", definition.0, sockets.0);
                    entity.insert((definition, sockets));
                }
                ChestPart::Corruption(Corruption::Mark) => {
                    entity.insert(Corrupted);
                }
                ChestPart::Corruption(Corruption::Reforge { power }) => {
                    let power = ReforgePower(power);
                    println!("Reforged with power {}", power.0);
                    entity.insert(power);
                }
                ChestPart::Corruption(Corruption::WhiteSockets(amount)) => {
                    let sockets = WhiteSockets(amount);
                    println!("Whitened {} sockets", sockets.0);
                    entity.insert(sockets);
                }
                ChestPart::Corruption(Corruption::Implicit { modifier, value }) => {
                    let implicit = Implicit { modifier, value };
                    println!("Implicit: {} = {}", implicit.modifier, implicit.value);
                    entity.insert(implicit);
                }
            }
        }
        exit.write(AppExit::Success);
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
        LootPlugin::default().with_sources(["corrupted_chest.loot"]),
    ))
    .add_loot_function("item", item)
    .add_loot_function("reforge", reforge)
    .add_loot_function("sockets.whiten", whiten_sockets)
    .add_loot_function("implicit", implicit)
    .add_loot_function("corrupted", || Corruption::Mark)
    .add_loot_output::<ChestPart>()
    .add_systems(Startup, request_chest)
    .add_systems(Update, open_chest);
    app
}
