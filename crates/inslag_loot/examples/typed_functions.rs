use bevy_app::{App, AppExit, ScheduleRunnerPlugin, Startup, TaskPoolPlugin, Update};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use inslag_loot::prelude::*;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, LootOutput)]
struct Currency {
    amount: u32,
}

#[derive(LootArgs)]
struct CurrencyArgs {
    #[loot(primary)]
    amount: u32,
}

#[derive(Resource, Default)]
struct Balance(u32);

fn currency(args: CurrencyArgs) -> Currency {
    Currency {
        amount: args.amount,
    }
}

fn request_reward(mut requests: MessageWriter<LootRequest<Currency>>) {
    requests.write(LootRequest::new("reward").input("bonus", 15u32));
}

fn grant_reward(
    mut finished: MessageReader<LootFinished<Currency>>,
    mut balance: ResMut<Balance>,
    mut exit: MessageWriter<AppExit>,
) {
    for roll in finished.read() {
        match &roll.result {
            Ok(currencies) => {
                for currency in currencies {
                    balance.0 += currency.amount;
                }
                println!("Balance: {}", balance.0);
                exit.write(AppExit::Success);
            }
            Err(error) => {
                eprintln!("{error}");
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
        LootPlugin::default().with_sources(["typed_functions.loot"]),
    ))
    .init_resource::<Balance>()
    .add_loot_function("currency", currency)
    .add_loot_output::<Currency>()
    .add_systems(Startup, request_reward)
    .add_systems(Update, grant_reward);
    app
}

fn main() -> AppExit {
    app()
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(1)))
        .run()
}
