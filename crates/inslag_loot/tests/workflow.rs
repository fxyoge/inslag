use bevy_app::{App, Startup, TaskPoolPlugin, Update};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use inslag_loot::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

fn app(files: &[(&str, &str)], paths: &[&str]) -> (App, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    for (path, text) in files {
        std::fs::write(directory.path().join(path), text).unwrap();
    }
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            file_path: directory.path().to_string_lossy().into_owned(),
            ..Default::default()
        },
        LootPlugin::default().with_sources(paths.iter().map(|path| path.to_string())),
    ));
    (app, directory)
}

fn until(app: &mut App, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready(app) {
        assert!(Instant::now() < deadline, "loot request did not finish");
        app.update();
        std::thread::yield_now();
    }
}

fn roll<T: LootOutput + Clone>(app: &mut App, request: LootRequest<T>) -> LootFinished<T> {
    let id = request.id();
    let mut cursor = app
        .world()
        .resource::<Messages<LootFinished<T>>>()
        .get_cursor_current();
    app.world_mut().write_message(request);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "loot request did not finish");
        app.update();
        if let Some(finished) = cursor
            .read(app.world().resource::<Messages<LootFinished<T>>>())
            .next()
        {
            assert_eq!(finished.id, id);
            return finished.clone();
        }
        std::thread::yield_now();
    }
}

#[derive(Clone, Debug, PartialEq, LootValue)]
enum Rarity {
    Normal,
    #[loot(rename = "rare")]
    Rare,
}

#[derive(Clone, Debug, PartialEq, LootOutput)]
struct Item {
    definition: String,
    rarity: Rarity,
}

#[derive(LootArgs)]
struct ItemArgs {
    #[loot(primary, rename = "item")]
    definition: String,
    #[loot(default = Rarity::Normal)]
    rarity: Rarity,
}

fn item(args: ItemArgs) -> Item {
    Item {
        definition: args.definition,
        rarity: args.rarity,
    }
}

#[derive(Clone, Debug, PartialEq, LootOutput)]
struct Currency(u32);

#[derive(LootArgs)]
struct CurrencyArgs {
    amount: u32,
}

#[derive(Resource)]
struct Multiplier(u32);

fn currency(args: CurrencyArgs, multiplier: Res<Multiplier>) -> Currency {
    Currency(args.amount * multiplier.0)
}

#[derive(Clone, Debug, PartialEq, LootOutput)]
enum Reward {
    Item(Item),
    Currency(Currency),
}

#[derive(Resource, Default)]
struct Received(Vec<LootFinished<Reward>>);

#[derive(Resource, Default)]
struct Issued(Option<RollId>);

fn request_rewards(mut requests: MessageWriter<LootRequest<Reward>>, mut issued: ResMut<Issued>) {
    let request = LootRequest::new("rewards")
        .input("definition", "sword")
        .input("kind", Rarity::Rare)
        .input("gold", 25u32);
    issued.0 = Some(request.id());
    requests.write(request);
}

fn receive_rewards(
    mut finished: MessageReader<LootFinished<Reward>>,
    mut received: ResMut<Received>,
) {
    received.0.extend(finished.read().cloned());
}

#[test]
fn configure_request_receive_needs_no_readiness_or_ordering_code() {
    let (mut app, _directory) = app(
        &[
            (
                "rewards.loot",
                "table \"rewards\"\n  #items definition=$definition kind=$kind\n  @currency amount=$gold\n",
            ),
            (
                "items.loot",
                "table \"items\"\n  @item item=$definition rarity=$kind\n",
            ),
        ],
        &["rewards.loot", "items.loot"],
    );
    app.add_loot_function("item", item)
        .add_loot_function("currency", currency)
        .add_loot_output::<Reward>()
        .insert_resource(Multiplier(2))
        .init_resource::<Issued>()
        .init_resource::<Received>()
        .add_systems(Startup, request_rewards)
        .add_systems(Update, receive_rewards);
    until(&mut app, |app| {
        !app.world().resource::<Received>().0.is_empty()
    });
    let received = &app.world().resource::<Received>().0;
    assert_eq!(received.len(), 1);
    assert_eq!(Some(received[0].id), app.world().resource::<Issued>().0);
    assert_eq!(
        received[0].result.as_ref().unwrap(),
        &[
            Reward::Item(Item {
                definition: "sword".into(),
                rarity: Rarity::Rare
            }),
            Reward::Currency(Currency(50)),
        ]
    );
    for _ in 0..8 {
        app.update();
    }
    assert_eq!(app.world().resource::<Received>().0.len(), 1);
}

#[test]
fn luau_context_aliases_and_typed_effects_share_a_bag_without_ambiguity() {
    use inslag_luau::LuauValue;
    #[derive(Clone, Debug, PartialEq, LootOutput)]
    enum Inspected {
        Item(Item),
        Context(LuauValue),
    }
    let (mut app, _directory) = app(
        &[(
            "inspected.loot",
            "table \"root\"\n  @context as=documents\n  choose\n    @item item=sword when=\"documents[1].complete and documents[1].quality > minimum\"\n    @item item=broken when=\"not documents[1].complete\"\n",
        )],
        &["inspected.loot"],
    );
    app.add_loot_function("context", || {
        LuauValue::Sequence(vec![LuauValue::Record(
            [
                ("complete".into(), LuauValue::Boolean(true)),
                ("quality".into(), LuauValue::Number(2.)),
            ]
            .into(),
        )])
    })
    .add_loot_function("item", item)
    .add_loot_output::<Inspected>();
    let finished = roll(
        &mut app,
        LootRequest::<Inspected>::new("root").input("minimum", 1.),
    );
    let results = finished.result.unwrap();
    assert_eq!(results.len(), 2);
    assert!(matches!(&results[0], Inspected::Context(_)));
    assert!(matches!(&results[1], Inspected::Item(item) if item.definition == "sword"));
}

#[test]
fn one_document_context_and_nested_tables_return_only_completed_operations() {
    #[derive(Component)]
    struct Quality(f64);
    #[derive(Clone, Debug, PartialEq, LootArgs, LootOutput)]
    struct SetQuality {
        target: Reference,
        #[loot(primary)]
        value: f64,
    }
    let (mut app, _directory) = app(
        &[(
            "operations.loot",
            r#"table "root"
  #changes document=$document
table "changes"
  choose when="document.quality > 0"
    #qualities document=$document
table "qualities" using=document.set-quality target=$document
  choose exclude="document.quality"
    1
    2
table "exclusions" using=document.set-quality target=$document
  choose exclude="{document.quality, nil, 3}"
    1
    2
    3 weight=999
table "failed"
  #changes document=$document
  nothing when="document.quality.invalid"
table "suspended"
  @document.contextual target=$document value=3
  #changes document=$document
"#,
        )],
        &["operations.loot"],
    );
    app.add_loot_context(
        "document",
        |reference: &Reference,
         path: &[inslag_luau::LuauValue],
         qualities: Query<&Quality>|
         -> Result<ReadOnlyValue, String> {
            let entity = Entity::try_from_bits(reference.id).ok_or("invalid reference")?;
            let quality = qualities.get(entity).map_err(|_| "missing document")?;
            Ok(match path {
                [] => ReadOnlyValue::View,
                [inslag_luau::LuauValue::String(field)] if field == "quality" => {
                    ReadOnlyValue::Value(inslag_luau::LuauValue::Number(quality.0))
                }
                _ => ReadOnlyValue::Value(inslag_luau::LuauValue::Nil),
            })
        },
    )
    .add_loot_function("document.set-quality", |operation: SetQuality| operation)
    .add_loot_function(
        "document.contextual",
        |operation: SetQuality, qualities: Query<&Quality>| -> Result<SetQuality, String> {
            qualities
                .get(Entity::try_from_bits(operation.target.id).ok_or("invalid reference")?)
                .map_err(|_| "missing document")?;
            Ok(operation)
        },
    )
    .add_loot_output::<SetQuality>();
    let first = app.world_mut().spawn(Quality(1.)).id();
    let second = app.world_mut().spawn(Quality(2.)).id();
    let request = |table, entity: Entity| {
        LootRequest::<SetQuality>::new(table)
            .context("document")
            .input("document", Reference::new("document", entity.to_bits()))
            .seed(1)
    };
    let mutations = roll(&mut app, request("root", first)).result.unwrap();
    assert_eq!(mutations.len(), 1);
    assert_eq!(
        mutations[0],
        SetQuality {
            target: Reference::new("document", first.to_bits()),
            value: 2.
        }
    );
    assert_eq!(
        roll(&mut app, request("root", second)).result.unwrap()[0],
        SetQuality {
            target: Reference::new("document", second.to_bits()),
            value: 1.
        }
    );
    assert_eq!(app.world().get::<Quality>(first).unwrap().0, 1.);
    assert_eq!(app.world().get::<Quality>(second).unwrap().0, 2.);
    assert_eq!(
        roll(&mut app, request("exclusions", first)).result.unwrap()[0].value,
        2.
    );
    assert_eq!(
        roll(&mut app, request("exclusions", second))
            .result
            .unwrap()[0]
            .value,
        1.
    );
    let suspended = roll(&mut app, request("suspended", first)).result.unwrap();
    assert_eq!(suspended.len(), 2);
    assert_eq!(suspended[0].value, 3.);
    assert_eq!(suspended[1].value, 2.);
    assert!(
        roll(&mut app, request("failed", first))
            .result
            .unwrap_err()
            .to_string()
            .contains("when expression failed")
    );
    app.world_mut().get_mut::<Quality>(first).unwrap().0 = 2.;
    assert_eq!(
        roll(&mut app, request("root", first)).result.unwrap()[0].value,
        1.
    );
    assert!(
        roll(&mut app, request("root", first).context("unknown"))
            .result
            .unwrap_err()
            .to_string()
            .contains("unknown loot context")
    );
}

#[test]
fn reference_contexts_reject_deferred_world_mutations() {
    let (mut app, _directory) = app(&[("root.loot", "table \"root\"\n  1\n")], &["root.loot"]);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.add_loot_context(
            "invalid",
            |_: &Reference,
             _: &[inslag_luau::LuauValue],
             _: Commands|
             -> Result<ReadOnlyValue, String> { Ok(ReadOnlyValue::View) },
        );
    }));
    assert!(result.is_err());
}

#[test]
fn all_preparation_failures_are_terminal_results_with_diagnostics() {
    for (files, paths, expected) in [
        (vec![], vec!["missing.loot"], "missing.loot"),
        (
            vec![("bad.loot", "table \"root\"\n  @unknown\n")],
            vec!["bad.loot"],
            "bad.loot:2:",
        ),
        (
            vec![("bad.loot", "not a table")],
            vec!["bad.loot"],
            "bad.loot:1:",
        ),
        (
            vec![
                ("a.loot", "table \"root\"\n  1\n"),
                ("b.loot", "table \"root\"\n  2\n"),
            ],
            vec!["a.loot", "b.loot"],
            "duplicate table",
        ),
        (vec![], vec![], "no loot sources configured"),
    ] {
        let (mut app, _directory) = app(&files, &paths);
        app.add_loot_output::<i64>();
        let finished = roll(&mut app, LootRequest::<i64>::new("root"));
        assert!(finished.result.unwrap_err().to_string().contains(expected));
        let mut cursor = app
            .world()
            .resource::<Messages<LootFinished<i64>>>()
            .get_cursor_current();
        for _ in 0..8 {
            app.update();
            assert_eq!(
                cursor
                    .read(app.world().resource::<Messages<LootFinished<i64>>>())
                    .count(),
                0
            );
        }
    }
}

#[test]
fn unknown_tables_and_missing_inputs_are_request_errors() {
    let (mut app, _directory) = app(
        &[("rewards.loot", "table \"root\"\n  @item item=$item\n")],
        &["rewards.loot"],
    );
    app.add_loot_function("item", item)
        .add_loot_output::<Item>();
    for (table, expected) in [("missing", "unknown table"), ("root", "requires input")] {
        let finished = roll(&mut app, LootRequest::<Item>::new(table));
        assert!(finished.result.unwrap_err().to_string().contains(expected));
    }
    let finished = roll(
        &mut app,
        LootRequest::<Item>::new("root").input("item", "sword"),
    );
    assert_eq!(
        finished.result.unwrap(),
        [Item {
            definition: "sword".into(),
            rarity: Rarity::Normal
        }]
    );
}

#[test]
fn output_validation_happens_before_any_generator_runs() {
    let (mut app, _directory) = app(
        &[("rewards.loot", "table \"root\"\n  @number\n  @text\n")],
        &["rewards.loot"],
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    app.add_loot_output::<i64>()
        .add_loot_function("number", move || {
            observed.fetch_add(1, Ordering::Relaxed);
            1i64
        })
        .add_loot_function("text", || "wrong output".to_string());
    let finished = roll(&mut app, LootRequest::<i64>::new("root"));
    assert!(
        finished
            .result
            .unwrap_err()
            .to_string()
            .contains("not supported")
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[test]
fn missing_generator_context_is_an_error_instead_of_a_stuck_roll() {
    let (mut app, _directory) = app(
        &[("rewards.loot", "table \"root\"\n  @currency amount=1\n")],
        &["rewards.loot"],
    );
    app.add_loot_function("currency", currency)
        .add_loot_output::<Currency>();
    let finished = roll(&mut app, LootRequest::<Currency>::new("root"));
    let error = finished.result.unwrap_err().to_string();
    assert!(error.contains("Multiplier"), "{error}");
    assert!(error.contains("rewards.loot:2:"), "{error}");
}

#[test]
fn typed_channels_registration_and_ids_do_not_duplicate_or_mix_results() {
    let (mut app, _directory) = app(
        &[(
            "rewards.loot",
            "table \"number\"\n  42\ntable \"text\"\n  hello\n",
        )],
        &["rewards.loot"],
    );
    app.add_loot_output::<i64>()
        .add_loot_output::<String>()
        .add_loot_output::<i64>();
    let owner = app.world_mut().spawn_empty().id();
    let number = LootRequest::<i64>::new("number").owner(owner);
    let number_id = number.id();
    let text = LootRequest::<String>::new("text").owner(owner);
    let text_id = text.id();
    assert_ne!(number_id, text_id);
    let mut numbers = app
        .world()
        .resource::<Messages<LootFinished<i64>>>()
        .get_cursor_current();
    let mut strings = app
        .world()
        .resource::<Messages<LootFinished<String>>>()
        .get_cursor_current();
    app.world_mut().write_message(number);
    app.world_mut().write_message(text);
    let (mut number_results, mut string_results) = (Vec::new(), Vec::new());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline);
        app.update();
        number_results.extend(
            numbers
                .read(app.world().resource::<Messages<LootFinished<i64>>>())
                .cloned(),
        );
        string_results.extend(
            strings
                .read(app.world().resource::<Messages<LootFinished<String>>>())
                .cloned(),
        );
        if !number_results.is_empty() && !string_results.is_empty() {
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(number_results.len(), 1);
    assert_eq!(string_results.len(), 1);
    assert_eq!(number_results[0].id, number_id);
    assert_eq!(string_results[0].id, text_id);
    assert_eq!(number_results[0].owner, Some(owner));
    assert_eq!(string_results[0].owner, Some(owner));
    assert_eq!(number_results[0].result.as_ref().unwrap(), &[42]);
    assert_eq!(string_results[0].result.as_ref().unwrap(), &["hello"]);
}

#[derive(Component)]
struct Health(i64);

#[derive(LootArgs)]
struct ReadHealth {
    #[loot(reference = "actor")]
    target: Reference,
}

fn health(args: ReadHealth, health: Query<&Health>) -> Result<i64, String> {
    health
        .get(Entity::from_bits(args.target.id))
        .map(|health| health.0)
        .map_err(|error| error.to_string())
}

#[test]
fn references_allow_generators_to_query_live_components() {
    let (mut app, _directory) = app(
        &[("rewards.loot", "table \"root\"\n  @health target=$target\n")],
        &["rewards.loot"],
    );
    app.add_loot_function("health", health)
        .add_loot_output::<i64>();
    let target = app.world_mut().spawn(Health(37)).id();
    let finished = roll(
        &mut app,
        LootRequest::<i64>::new("root").input("target", Reference::new("actor", target.to_bits())),
    );
    assert_eq!(finished.result.unwrap(), [37]);
    assert_eq!(app.world().get::<Health>(target).unwrap().0, 37);
}

fn draw(context: &mut LootFunctionContext, _: Res<Multiplier>) -> i64 {
    context.rng().i64(0..1_000_000)
}

#[test]
fn explicit_seeds_are_independent_of_request_ids_and_other_rolls() {
    let (mut app, _directory) = app(
        &[("random.loot", "table \"root\"\n  @draw\n  1..1000000\n")],
        &["random.loot"],
    );
    app.insert_resource(Multiplier(1))
        .add_loot_function("draw", draw)
        .add_loot_output::<i64>();
    let mut expected = HashMap::new();
    let mut outcomes = HashSet::new();
    for seed in 0..16 {
        let values = roll(&mut app, LootRequest::<i64>::new("root").seed(seed))
            .result
            .unwrap();
        outcomes.insert(values.clone());
        expected.insert(seed, values);
    }
    assert!(outcomes.len() > 1);
    for seed in (0..16).rev() {
        let values = roll(&mut app, LootRequest::<i64>::new("root").seed(seed))
            .result
            .unwrap();
        assert_eq!(values, expected[&seed]);
    }
}
