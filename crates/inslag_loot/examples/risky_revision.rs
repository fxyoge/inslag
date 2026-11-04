use bevy_app::{App, AppExit, ScheduleRunnerPlugin, Startup, TaskPoolPlugin, Update};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use inslag_loot::prelude::*;
use std::{collections::BTreeSet, time::Duration};

#[derive(Clone, Debug, PartialEq, LootArgs, LootOutput)]
struct Attribute {
    field: String,
    multiplier: f64,
}

#[derive(Clone, Debug, PartialEq, LootOutput)]
struct Tag(String);

#[derive(Clone, Debug, PartialEq, LootOutput)]
struct ItemSpec(String);

#[derive(Clone, Debug, PartialEq, LootOutput)]
enum Revision {
    Attribute(Attribute),
    Tag(Tag),
    Item(ItemSpec),
}

#[derive(Component, Debug)]
struct Quality(f64);

#[derive(Component, Debug)]
struct Material(f64);

#[derive(Component, Default, Debug)]
struct Tags(BTreeSet<String>);

#[derive(Component, Debug)]
struct DefinitionId(String);

#[derive(LootArgs)]
struct TagArgs {
    tag: String,
}

#[derive(LootArgs)]
struct ItemArgs {
    item: String,
}

fn setup(mut commands: Commands) {
    commands.spawn((Quality(1.), Material(1.), Tags::default()));
}

fn request_revisions(
    submissions: Query<Entity, Added<Tags>>,
    mut requests: MessageWriter<LootRequest<Revision>>,
) {
    for entity in &submissions {
        requests.write(LootRequest::new("risky-revision").seed(0).owner(entity));
    }
}

fn revise_submission(
    mut commands: Commands,
    mut finished: MessageReader<LootFinished<Revision>>,
    mut submissions: Query<(&mut Quality, &mut Material, &mut Tags)>,
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
        let Some(owner) = roll.owner else { continue };
        let Ok((mut quality, mut material, mut tags)) = submissions.get_mut(owner) else {
            continue;
        };
        for revision in generated.iter().cloned() {
            match revision {
                Revision::Attribute(attribute) => match attribute.field.as_str() {
                    "quality" => quality.0 *= attribute.multiplier,
                    "material" => material.0 *= attribute.multiplier,
                    _ => panic!("unknown submission attribute"),
                },
                Revision::Tag(tag) => {
                    tags.0.insert(tag.0);
                }
                Revision::Item(item) => {
                    let definition = DefinitionId(item.0);
                    println!("Received {}", definition.0);
                    commands.spawn(definition);
                }
            }
        }
        println!(
            "Quality: {}, material: {}, tags: {:?}",
            quality.0, material.0, tags.0
        );
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
        LootPlugin::default().with_sources(["risky_revision.loot"]),
    ))
    .add_loot_function("attribute", |args: Attribute| args)
    .add_loot_function("tag", |args: TagArgs| Tag(args.tag))
    .add_loot_function("item", |args: ItemArgs| ItemSpec(args.item))
    .add_loot_output::<Revision>()
    .add_systems(Startup, setup)
    .add_systems(Update, (request_revisions, revise_submission));
    app
}

fn main() -> AppExit {
    app()
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(1)))
        .run()
}
