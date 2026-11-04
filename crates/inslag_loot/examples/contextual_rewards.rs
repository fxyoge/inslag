use bevy_app::{App, AppExit, ScheduleRunnerPlugin, Startup, TaskPoolPlugin, Update};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use inslag_loot::prelude::*;
use std::{collections::BTreeMap, time::Duration};

#[derive(Clone, Debug, PartialEq, LootOutput)]
struct ItemSpec {
    definition: String,
    rarity: String,
}

#[derive(Clone, Debug, PartialEq, LootOutput)]
struct Currency {
    currency: String,
    amount: u32,
}

#[derive(Clone, Debug, PartialEq, LootOutput)]
enum Reward {
    Item(ItemSpec),
    Currency(Currency),
}

#[derive(Component, Debug)]
struct DefinitionId(String);

#[derive(Component, Debug)]
struct Rarity(String);

#[derive(Resource, Default, Debug)]
struct Wallet(BTreeMap<String, u32>);

#[derive(Resource)]
struct RewardContext {
    blessed: bool,
    gold: u32,
}

#[derive(LootArgs)]
struct SpawnItem {
    #[loot(primary)]
    item: String,
    #[loot(default = "normal".into())]
    rarity: String,
}

#[derive(LootArgs)]
struct CurrencyArgs {
    currency: String,
    amount: u32,
}

fn item(args: SpawnItem) -> ItemSpec {
    ItemSpec {
        definition: args.item,
        rarity: args.rarity,
    }
}

fn currency(args: CurrencyArgs) -> Currency {
    Currency {
        currency: args.currency,
        amount: args.amount,
    }
}

fn request_rewards(context: Res<RewardContext>, mut requests: MessageWriter<LootRequest<Reward>>) {
    requests.write(
        LootRequest::new("rewards")
            .input("blessed", context.blessed)
            .input("gold", context.gold),
    );
}

fn grant_rewards(
    mut commands: Commands,
    mut finished: MessageReader<LootFinished<Reward>>,
    mut wallet: ResMut<Wallet>,
    mut exit: MessageWriter<AppExit>,
) {
    for roll in finished.read() {
        let rewards = match &roll.result {
            Ok(rewards) => rewards,
            Err(error) => {
                eprintln!("{error}");
                exit.write(AppExit::error());
                continue;
            }
        };
        for reward in rewards {
            match reward {
                Reward::Item(item) => {
                    let definition = DefinitionId(item.definition.clone());
                    let rarity = Rarity(item.rarity.clone());
                    println!("Received {} ({})", definition.0, rarity.0);
                    commands.spawn((definition, rarity));
                }
                Reward::Currency(currency) => {
                    *wallet.0.entry(currency.currency.clone()).or_default() += currency.amount;
                }
            }
        }
        println!("Wallet: {:?}", wallet.0);
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
        LootPlugin::default().with_sources(["contextual_rewards.loot"]),
    ))
    .insert_resource(RewardContext {
        blessed: true,
        gold: 50,
    })
    .init_resource::<Wallet>()
    .add_loot_function("item", item)
    .add_loot_function("currency", currency)
    .add_loot_output::<Reward>()
    .add_systems(Startup, request_rewards)
    .add_systems(Update, grant_rewards);
    app
}
