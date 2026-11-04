use crate::*;
use bevy_app::{App, TaskPoolPlugin, Update};
use bevy_asset::AssetPlugin;
use bevy_ecs::prelude::*;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone, Debug, PartialEq, LootOutput)]
struct Item {
    definition: String,
    rarity: Rarity,
    amount: u32,
    note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, LootValue)]
enum Rarity {
    Normal,
    Rare,
    #[loot(rename = "one-of-a-kind")]
    Unique,
}

#[derive(LootArgs)]
struct SpawnItem {
    #[loot(primary, rename = "item")]
    definition: String,
    #[loot(default = Rarity::Normal)]
    rarity: Rarity,
    #[loot(default = 1)]
    amount: u32,
    note: Option<String>,
}

fn spawn_item(args: SpawnItem) -> Item {
    Item {
        definition: args.definition,
        rarity: args.rarity,
        amount: args.amount,
        note: args.note,
    }
}

fn item_app() -> App {
    let mut app = App::new();
    app.add_loot_function("item.spawn", spawn_item);
    app
}

fn compile(app: &App, text: &str) -> Arc<Catalog> {
    app.world()
        .resource::<LootRegistry>()
        .compile([SourceDocument::new("typed.loot", text)])
        .unwrap()
}

#[test]
fn argument_types_supply_the_signature_defaults_primary_and_names() {
    let app = item_app();
    let catalog = compile(
        &app,
        "table \"root\" using=item.spawn\n  sword\n  axe rarity=rare amount=2 note=gift\n  wand rarity=one-of-a-kind\n",
    );
    let table = catalog.table::<Item>("root").unwrap();
    assert_eq!(table.name(), "root");
    let values: Vec<Item> = app
        .world()
        .resource::<LootRegistry>()
        .generate(&table, BTreeMap::new(), 0)
        .unwrap();
    assert_eq!(
        values,
        [
            Item {
                definition: "sword".into(),
                rarity: Rarity::Normal,
                amount: 1,
                note: None
            },
            Item {
                definition: "axe".into(),
                rarity: Rarity::Rare,
                amount: 2,
                note: Some("gift".into())
            },
            Item {
                definition: "wand".into(),
                rarity: Rarity::Unique,
                amount: 1,
                note: None
            },
        ]
    );
}

#[test]
fn invalid_literals_are_rejected_before_a_roll() {
    let app = item_app();
    let registry = app.world().resource::<LootRegistry>();
    for args in [
        "",
        "item=1",
        "item=sword rarity=raer",
        "item=sword amount=-1",
        "item=sword amount=0..4294967296",
        "item=sword typo=true",
    ] {
        let error = registry
            .compile([SourceDocument::new(
                "bad.loot",
                format!("table \"root\"\n  @item.spawn {args}\n"),
            )])
            .unwrap_err();
        assert_eq!(error.location.line, 2, "{args}: {error}");
    }
}

#[test]
fn dynamic_enums_and_unsigned_values_are_checked_before_the_handler() {
    let app = item_app();
    for (argument, value) in [
        ("rarity", Value::from("raer")),
        ("amount", Value::from(-1i64)),
        ("amount", Value::from(i64::MAX)),
    ] {
        let catalog = compile(
            &app,
            &format!("table \"root\"\n  @item.spawn item=sword {argument}=$input\n"),
        );
        let error = app
            .world()
            .resource::<LootRegistry>()
            .generate(
                &catalog.table::<Item>("root").unwrap(),
                BTreeMap::from([("input".into(), value)]),
                0,
            )
            .unwrap_err();
        assert_eq!(error.location.line, 2);
        assert!(error.message.contains(argument));
    }
}

#[test]
fn defaults_are_evaluated_only_when_omitted_and_once_per_call() {
    static DEFAULTS: AtomicUsize = AtomicUsize::new(0);
    fn next_default() -> i64 {
        DEFAULTS.fetch_add(1, Ordering::SeqCst) as i64
    }
    #[derive(LootArgs)]
    struct Args {
        #[loot(default = next_default())]
        value: i64,
        #[loot(default)]
        enabled: bool,
        #[loot(default = Some("fallback".into()))]
        note: Option<String>,
    }
    let mut app = App::new();
    app.add_loot_function("test", |args: Args| {
        assert!(!args.enabled);
        assert_eq!(args.note.as_deref(), Some("fallback"));
        args.value
    });
    let catalog = compile(&app, "table \"root\"\n  @test value=99\n  @test\n  @test\n");
    let values = app
        .world()
        .resource::<LootRegistry>()
        .generate(&catalog.table::<i64>("root").unwrap(), BTreeMap::new(), 0)
        .unwrap();
    assert_eq!(values, [99, 0, 1]);
    assert_eq!(DEFAULTS.load(Ordering::SeqCst), 2);
}

#[test]
fn plain_optional_vector_and_fallible_returns_flatten_without_wrappers() {
    let mut app = App::new();
    app.add_loot_function("one", || 1i64)
        .add_loot_function("some", || Some(2i64))
        .add_loot_function("none", || None::<i64>)
        .add_loot_function("many", || vec![3i64, 4])
        .add_loot_function("empty", Vec::<i64>::new)
        .add_loot_function("fallible", || Ok::<_, std::io::Error>(vec![5i64, 6]))
        .add_loot_function("optional", || Ok::<_, std::io::Error>(Some(7i64)))
        .add_loot_function("unit", || ());
    let catalog = compile(
        &app,
        "table \"root\"\n  @one\n  @some\n  @none\n  @many\n  @empty\n  @fallible\n  @optional\n  @unit\n",
    );
    let values: Vec<i64> = app
        .world()
        .resource::<LootRegistry>()
        .generate(&catalog.table::<i64>("root").unwrap(), BTreeMap::new(), 0)
        .unwrap();
    assert_eq!(values, [1, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn aliases_still_require_exactly_one_value() {
    let mut app = App::new();
    app.add_loot_function("none", || None::<i64>)
        .add_loot_function("many", || vec![1i64, 2]);
    for (function, count) in [("none", 0), ("many", 2)] {
        let catalog = compile(&app, &format!("table \"root\"\n  @{function} as=value\n"));
        let error = app
            .world()
            .resource::<LootRegistry>()
            .generate(&catalog.table::<i64>("root").unwrap(), BTreeMap::new(), 0)
            .unwrap_err();
        assert!(error.message.contains(&format!("got {count}")));
    }
}

#[test]
fn fallible_handlers_keep_error_locations_and_publish_no_partial_vector() {
    let mut app = App::new();
    app.add_loot_function("one", || 1i64)
        .add_loot_function("fail", || {
            Err::<Vec<i64>, _>(std::io::Error::other("ineligible"))
        });
    let catalog = compile(&app, "table \"root\"\n  @one\n  @fail\n");
    let error = app
        .world()
        .resource::<LootRegistry>()
        .generate(&catalog.table::<i64>("root").unwrap(), BTreeMap::new(), 0)
        .unwrap_err();
    assert_eq!(error.location.line, 3);
    assert!(error.message.contains("ineligible"));
}

#[test]
fn selecting_a_root_checks_all_reachable_outputs_without_invoking_handlers() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut app = App::new();
    app.add_loot_function("number", move || {
        observed.fetch_add(1, Ordering::SeqCst);
        1i64
    })
    .add_loot_function("text", || "text".to_string());
    let catalog = compile(
        &app,
        "table \"root\"\n  @number\n  #child\ntable \"child\"\n  choose\n    @text weight=0\n    @number\ntable \"numbers\"\n  @number\n",
    );
    let error = catalog.table::<i64>("root").unwrap_err();
    assert_eq!(error.location.line, 6);
    assert!(error.message.contains("not supported"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(catalog.table::<i64>("numbers").is_ok());
    assert!(catalog.table::<i64>("missing").is_err());
}

#[test]
fn enum_bags_preserve_scalar_types_and_do_not_silently_filter_results() {
    #[derive(Debug, PartialEq, LootOutput)]
    enum Part {
        Count(u32),
        Rarity(Rarity),
    }
    let mut app = App::new();
    app.add_loot_function("count", || 3u32)
        .add_loot_function("rarity", || Rarity::Rare);
    let catalog = compile(&app, "table \"root\"\n  @count\n  @rarity\n");
    let values = app
        .world()
        .resource::<LootRegistry>()
        .generate(&catalog.table::<Part>("root").unwrap(), BTreeMap::new(), 0)
        .unwrap();
    assert_eq!(values, [Part::Count(3), Part::Rarity(Rarity::Rare)]);
    let literals = compile(&app, "table \"root\"\n  2\n  rare\n");
    assert!(literals.table::<Part>("root").is_ok());
    assert!(literals.table::<Rarity>("root").is_err());
}

#[test]
fn a_handler_can_also_return_a_vector_of_mixed_enum_variants() {
    #[derive(Debug, PartialEq, LootOutput)]
    enum Part {
        Number(i64),
        Text(String),
    }
    let mut app = App::new();
    app.add_loot_function("mixed", || -> Result<Vec<Part>, std::io::Error> {
        Ok(vec![
            Part::Number(1),
            Part::Text("hello".into()),
            Part::Number(2),
        ])
    });
    let catalog = compile(&app, "table \"root\"\n  @mixed\n");
    assert!(catalog.table::<i64>("root").is_err());
    let values = app
        .world()
        .resource::<LootRegistry>()
        .generate(&catalog.table::<Part>("root").unwrap(), BTreeMap::new(), 0)
        .unwrap();
    assert_eq!(
        values,
        [Part::Number(1), Part::Text("hello".into()), Part::Number(2)]
    );
}

#[test]
fn generic_argument_and_output_structs_are_supported() {
    #[derive(LootArgs)]
    struct Args<T> {
        value: T,
    }
    #[derive(Clone, Debug, PartialEq, LootOutput)]
    struct Wrapped<T>(T);
    let mut app = App::new();
    app.add_loot_function("generic", |args: Args<String>| Wrapped(args.value));
    let catalog = compile(&app, "table \"root\"\n  @generic value=hello\n");
    let values = app
        .world()
        .resource::<LootRegistry>()
        .generate(
            &catalog.table::<Wrapped<String>>("root").unwrap(),
            BTreeMap::new(),
            0,
        )
        .unwrap();
    assert_eq!(values, [Wrapped("hello".to_string())]);
}

#[test]
fn random_contexts_remain_seeded_and_independent_per_call() {
    let mut app = App::new();
    app.add_loot_function("random", |ctx: &mut LootFunctionContext| {
        ctx.rng().i64(0..10000)
    })
    .add_loot_function("scaled", |ctx: &mut LootFunctionContext, args: Scale| {
        ctx.rng().i64(0..10000) * args.amount
    });
    let catalog = compile(
        &app,
        "table \"root\"\n  @random\n  @random\n  @scaled amount=2\n",
    );
    let table = catalog.table::<i64>("root").unwrap();
    let registry = app.world().resource::<LootRegistry>();
    let first = registry.generate(&table, BTreeMap::new(), 42).unwrap();
    assert_eq!(
        first,
        registry.generate(&table, BTreeMap::new(), 42).unwrap()
    );
    assert_ne!(
        first,
        registry.generate(&table, BTreeMap::new(), 43).unwrap()
    );
}

#[test]
fn a_list_inside_a_domain_newtype_is_one_output() {
    #[derive(Clone, Debug, PartialEq, LootOutput)]
    struct Loadout(Vec<String>);
    let mut app = App::new();
    app.add_loot_function("loadout", || Loadout(vec!["sword".into(), "shield".into()]));
    let catalog = compile(&app, "table \"root\"\n  @loadout as=loadout\n");
    let values = app
        .world()
        .resource::<LootRegistry>()
        .generate(
            &catalog.table::<Loadout>("root").unwrap(),
            BTreeMap::new(),
            0,
        )
        .unwrap();
    assert_eq!(values, [Loadout(vec!["sword".into(), "shield".into()])]);
}

#[test]
fn nominal_reference_kinds_are_checked_before_the_handler() {
    #[derive(LootArgs)]
    struct Args {
        #[loot(reference = "item")]
        target: Reference,
    }
    let mut app = App::new();
    app.add_loot_function("target", |args: Args| args.target.id as i64);
    let catalog = compile(&app, "table \"root\"\n  @target target=$target\n");
    let error = app
        .world()
        .resource::<LootRegistry>()
        .generate(
            &catalog.table::<i64>("root").unwrap(),
            BTreeMap::from([("target".into(), Reference::new("actor", 1).into())]),
            0,
        )
        .unwrap_err();
    assert!(error.message.contains("target"));
}

#[derive(Resource)]
struct Multiplier(i64);

#[derive(Resource, Default)]
struct Outcomes {
    numbers: Vec<Vec<i64>>,
    strings: Vec<Vec<String>>,
}

fn collect(
    mut numbers: MessageReader<LootFinished<i64>>,
    mut strings: MessageReader<LootFinished<String>>,
    mut outcomes: ResMut<Outcomes>,
) {
    outcomes
        .numbers
        .extend(numbers.read().map(|result| result.result.clone().unwrap()));
    outcomes
        .strings
        .extend(strings.read().map(|result| result.result.clone().unwrap()));
}

#[test]
fn different_typed_roots_share_one_runtime_and_only_publish_to_their_own_channels() {
    #[derive(LootArgs)]
    struct Args {
        amount: i64,
    }
    fn multiply(args: Args, multiplier: Res<Multiplier>) -> i64 {
        args.amount * multiplier.0
    }
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        LootPlugin::default(),
    ))
    .insert_resource(Multiplier(3))
    .init_resource::<Outcomes>()
    .add_loot_output::<i64>()
    .add_loot_output::<String>()
    .add_loot_output::<i64>()
    .add_loot_function("multiply", multiply)
    .add_loot_function("text", || "hello".to_string())
    .add_systems(Update, collect);
    let catalog = compile(
        &app,
        "table \"number\"\n  @multiply amount=7\ntable \"text\"\n  @text\n",
    );
    app.world_mut().write_message(LootRequest::from_table(
        catalog.table::<i64>("number").unwrap(),
        0,
    ));
    app.world_mut().write_message(LootRequest::from_table(
        catalog.table::<String>("text").unwrap(),
        0,
    ));
    for _ in 0..4 {
        app.update();
    }
    let outcomes = app.world().resource::<Outcomes>();
    assert_eq!(outcomes.numbers, [vec![21]]);
    assert_eq!(outcomes.strings, [vec!["hello".to_string()]]);
    assert_eq!(app.world().resource::<Multiplier>().0, 3);
}

#[test]
fn typed_contextual_registration_still_rejects_deferred_commands() {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        LootPlugin::default(),
    ));
    fn bad(_: Commands) -> i64 {
        1
    }
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.add_loot_function("bad", bad);
    }));
    assert!(panic.is_err());
    assert!(
        app.world()
            .resource::<LootRegistry>()
            .signatures()
            .get("bad")
            .is_none()
    );
}

#[test]
fn an_asset_with_an_incompatible_output_fails_before_invoking_any_handler() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        LootPlugin::default(),
    ))
    .add_loot_output::<String>()
    .add_loot_function("number", move || {
        observed.fetch_add(1, Ordering::SeqCst);
        1i64
    });
    let catalog = compile(&app, "table \"root\"\n  @number\n");
    let mut cursor = app
        .world()
        .resource::<Messages<LootFinished<String>>>()
        .get_cursor_current();
    app.world_mut()
        .write_message(LootRequest::<String>::from_catalog(catalog, "root", 0));
    app.update();
    let result = cursor
        .read(app.world().resource::<Messages<LootFinished<String>>>())
        .next()
        .unwrap();
    assert!(
        result
            .result
            .as_ref()
            .unwrap_err()
            .message
            .contains("not supported")
    );
    assert!(result.trace.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[derive(Component)]
struct Points(i64);

#[derive(bevy_ecs::system::SystemParam)]
struct ReadContext<'w, 's> {
    multiplier: Res<'w, Multiplier>,
    points: Query<'w, 's, &'static Points>,
}

#[derive(LootArgs)]
struct Scale {
    amount: i64,
}

#[test]
fn one_registration_method_infers_every_handler_form() {
    fn random_args(ctx: &mut LootFunctionContext, args: Scale) -> i64 {
        args.amount + ctx.rng().i64(0..1)
    }
    fn resource_only(multiplier: Res<Multiplier>) -> i64 {
        multiplier.0
    }
    fn random_resource(ctx: &mut LootFunctionContext, multiplier: Res<Multiplier>) -> i64 {
        multiplier.0 + ctx.rng().i64(0..1)
    }
    fn resources(multiplier: Res<Multiplier>, points: Query<&Points>) -> i64 {
        multiplier.0 * points.single().unwrap().0
    }
    fn random_resources(
        ctx: &mut LootFunctionContext,
        multiplier: Res<Multiplier>,
        points: Query<&Points>,
    ) -> i64 {
        multiplier.0 * points.single().unwrap().0 + ctx.rng().i64(0..1)
    }
    fn args_resources(args: Scale, multiplier: Res<Multiplier>, points: Query<&Points>) -> i64 {
        args.amount * multiplier.0 + points.single().unwrap().0
    }
    fn random_args_resources(
        ctx: &mut LootFunctionContext,
        args: Scale,
        multiplier: Res<Multiplier>,
        points: Query<&Points>,
    ) -> i64 {
        args.amount * multiplier.0 + points.single().unwrap().0 + ctx.rng().i64(0..1)
    }
    fn custom_param(args: Scale, context: ReadContext) -> Result<Vec<i64>, std::io::Error> {
        Ok(vec![
            args.amount * context.multiplier.0,
            context.points.single().unwrap().0,
        ])
    }
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        LootPlugin::default(),
    ))
    .insert_resource(Multiplier(3))
    .add_loot_output::<i64>()
    .add_loot_function("random.args", random_args)
    .add_loot_function("resource", resource_only)
    .add_loot_function("random.resource", random_resource)
    .add_loot_function("resources", resources)
    .add_loot_function("random.resources", random_resources)
    .add_loot_function("args.resources", args_resources)
    .add_loot_function("random.args.resources", random_args_resources)
    .add_loot_function("custom", custom_param);
    let entity = app.world_mut().spawn(Points(5)).id();
    let catalog = compile(
        &app,
        "table \"root\"\n  @random.args amount=2\n  @resource\n  @random.resource\n  @resources\n  @random.resources\n  @args.resources amount=2\n  @random.args.resources amount=2\n  @custom amount=2\n",
    );
    let mut cursor = app
        .world()
        .resource::<Messages<LootFinished<i64>>>()
        .get_cursor_current();
    app.world_mut().write_message(LootRequest::from_table(
        catalog.table::<i64>("root").unwrap(),
        42,
    ));
    let mut finished = Vec::new();
    for _ in 0..16 {
        app.update();
        finished.extend(
            cursor
                .read(app.world().resource::<Messages<LootFinished<i64>>>())
                .cloned(),
        );
        if !finished.is_empty() {
            break;
        }
    }
    assert_eq!(finished.len(), 1);
    assert_eq!(
        finished[0].result.as_ref().unwrap(),
        &[2, 3, 3, 15, 15, 11, 11, 6, 5]
    );
    assert_eq!(app.world().get::<Points>(entity).unwrap().0, 5);
    assert_eq!(app.world().resource::<Multiplier>().0, 3);
}

#[test]
fn by_value_parameters_process_the_entire_batch_in_one_generator_update() {
    fn scale(args: Scale, multiplier: Res<Multiplier>) -> i64 {
        args.amount * multiplier.0
    }
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        LootPlugin::default(),
    ))
    .insert_resource(Multiplier(3))
    .add_loot_output::<i64>()
    .add_loot_function("scale", scale);
    let catalog = compile(&app, "table \"root\"\n  @scale amount=$amount\n");
    let table = catalog.table::<i64>("root").unwrap();
    let mut cursor = app
        .world()
        .resource::<Messages<LootFinished<i64>>>()
        .get_cursor_current();
    for amount in 0..64i64 {
        app.world_mut()
            .write_message(LootRequest::from_table(table.clone(), 42).input("amount", amount));
    }
    app.update();
    assert_eq!(
        cursor
            .read(app.world().resource::<Messages<LootFinished<i64>>>())
            .count(),
        0
    );
    app.update();
    let finished: Vec<_> = cursor
        .read(app.world().resource::<Messages<LootFinished<i64>>>())
        .collect();
    assert_eq!(finished.len(), 64);
    let values: Vec<_> = finished
        .iter()
        .map(|result| result.result.as_ref().unwrap()[0])
        .collect();
    assert_eq!(values, (0..64).map(|value| value * 3).collect::<Vec<_>>());
}

#[test]
fn resource_only_handlers_still_require_the_bevy_request_pipeline() {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        LootPlugin::default(),
    ))
    .insert_resource(Multiplier(3))
    .add_loot_function("resource", |multiplier: Res<Multiplier>| multiplier.0);
    let catalog = compile(&app, "table \"root\"\n  @resource\n");
    let error = app
        .world()
        .resource::<LootRegistry>()
        .generate(&catalog.table::<i64>("root").unwrap(), BTreeMap::new(), 0)
        .unwrap_err();
    assert!(error.message.contains("read-only Bevy context"));
}
