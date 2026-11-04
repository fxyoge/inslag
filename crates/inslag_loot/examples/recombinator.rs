use bevy_app::{App, AppExit, ScheduleRunnerPlugin, Startup, TaskPoolPlugin, Update};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use inslag_loot::prelude::*;
use std::{sync::Arc, time::Duration};

#[derive(Component, Clone, Debug, PartialEq)]
struct DefinitionId(String);
#[derive(Clone, Debug, PartialEq, LootOutput)]
struct BaseSpec {
    definition: String,
}
#[derive(Clone, Debug, PartialEq, LootOutput)]
struct Modifier {
    group: String,
    kind: String,
    value: i64,
    restricted_to: Option<String>,
}
#[derive(Component, Clone, Debug, PartialEq)]
struct Modifiers(Vec<Modifier>);

#[derive(Clone, Debug, PartialEq, LootOutput)]
enum RecombinationPart {
    Base(BaseSpec),
    Modifier(Modifier),
}

#[derive(LootArgs)]
struct SelectBase {
    #[loot(reference = "item")]
    item: Reference,
}

#[derive(LootArgs)]
struct SampleAffixes {
    #[loot(reference = "item")]
    left: Reference,
    #[loot(reference = "item")]
    right: Reference,
    base: Arc<BaseSpec>,
    kind: String,
    #[loot(primary)]
    amount: u32,
}

fn select_base(args: SelectBase, definitions: Query<&DefinitionId>) -> Result<BaseSpec, String> {
    let donor = donor_entity(args.item)?;
    let definition = definitions
        .get(donor)
        .map_err(|_| "donor no longer exists")?;
    Ok(BaseSpec {
        definition: definition.0.clone(),
    })
}

fn sample_affixes(
    ctx: &mut LootFunctionContext,
    args: SampleAffixes,
    modifiers: Query<&Modifiers>,
) -> Result<Vec<Modifier>, String> {
    let left = donor_entity(args.left)?;
    let right = donor_entity(args.right)?;
    if left == right {
        return Err("recombination requires distinct donors".into());
    }
    let mut pool = Vec::new();
    for donor in [left, right] {
        let source = modifiers.get(donor).map_err(|_| "donor no longer exists")?;
        pool.extend(
            source
                .0
                .iter()
                .filter(|modifier| {
                    modifier.kind == args.kind
                        && modifier
                            .restricted_to
                            .as_ref()
                            .is_none_or(|name| *name == args.base.definition)
                })
                .cloned(),
        );
    }
    let mut selected = Vec::new();
    for _ in 0..args.amount {
        if pool.is_empty() {
            return Err("not enough compatible modifiers".into());
        }
        let modifier = pool.remove(ctx.rng().usize(0..pool.len()));
        pool.retain(|candidate| candidate.group != modifier.group);
        selected.push(modifier);
    }
    Ok(selected)
}

fn donor_entity(reference: Reference) -> Result<Entity, String> {
    Entity::try_from_bits(reference.id).ok_or_else(|| "invalid donor reference".into())
}

#[derive(Component)]
struct Recombine {
    left: Entity,
    right: Entity,
    seed: u64,
}

fn spawn_donor(commands: &mut Commands, definition: &str, suffix: i64) -> Entity {
    commands
        .spawn((
            DefinitionId(definition.into()),
            Modifiers(
                [
                    ("life", "prefix", 100),
                    ("armor", "prefix", 50),
                    ("resist", "suffix", 20),
                    ("speed", "suffix", 5),
                ]
                .into_iter()
                .map(|(group, kind, value)| Modifier {
                    group: format!("{group}-{suffix}"),
                    kind: kind.into(),
                    value: value + suffix,
                    restricted_to: None,
                })
                .collect(),
            ),
        ))
        .id()
}

fn setup(mut commands: Commands) {
    let left = spawn_donor(&mut commands, "leather_tunic", 1);
    let right = spawn_donor(&mut commands, "plate_vest", 2);
    commands.spawn(Recombine {
        left,
        right,
        seed: 13,
    });
}

fn request_recombination(
    pending: Query<(Entity, &Recombine), Added<Recombine>>,
    mut requests: MessageWriter<LootRequest<RecombinationPart>>,
) {
    for (entity, pending) in &pending {
        requests.write(
            LootRequest::new("recombine")
                .seed(pending.seed)
                .input("left", Reference::new("item", pending.left.to_bits()))
                .input("right", Reference::new("item", pending.right.to_bits()))
                .owner(entity),
        );
    }
}

fn spawn_recombined_item(
    mut commands: Commands,
    mut finished: MessageReader<LootFinished<RecombinationPart>>,
    mut exit: MessageWriter<AppExit>,
) {
    for finished in finished.read() {
        if let Some(owner) = finished.owner {
            commands.entity(owner).despawn();
        }
        let generated = match &finished.result {
            Ok(generated) => generated,
            Err(error) => {
                eprintln!("Recombination failed: {error}");
                exit.write(AppExit::error());
                continue;
            }
        };
        let mut definition = None;
        let mut modifiers = Vec::new();
        for part in generated {
            match part {
                RecombinationPart::Base(base) => definition = Some(base.definition.clone()),
                RecombinationPart::Modifier(modifier) => modifiers.push(modifier.clone()),
            }
        }
        let definition = DefinitionId(definition.expect("recombination must select a base"));
        let modifiers = Modifiers(modifiers);
        println!("{}: {:?}", definition.0, modifiers.0);
        commands.spawn((definition, modifiers));
        exit.write(AppExit::Success);
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
        LootPlugin::default().with_sources(["recombinator.loot"]),
    ))
    .add_loot_function("base", select_base)
    .add_loot_function("affix.sample", sample_affixes)
    .add_loot_output::<RecombinationPart>()
    .add_systems(Update, (request_recombination, spawn_recombined_item));
    app
}

fn main() -> AppExit {
    app()
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(1)))
        .add_systems(Startup, setup)
        .run()
}
