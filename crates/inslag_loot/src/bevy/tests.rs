use super::*;
use crate::*;
use bevy_app::{App, TaskPoolPlugin};
use bevy_asset::AssetPlugin;
use std::sync::Arc;

#[derive(Resource)]
struct Input(i64);
#[derive(Component, Debug, PartialEq)]
struct Health(i64);
#[derive(Resource, Default)]
struct Observed(Vec<LootFinished<Value>>);
#[derive(Clone, Debug, PartialEq)]
struct Reward {
    amount: i64,
}

fn observe(mut messages: MessageReader<LootFinished<Value>>, mut observed: ResMut<Observed>) {
    observed.0.extend(messages.read().cloned());
}

fn fixture() -> App {
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        LootPlugin::default(),
    ))
    .add_loot_output::<Value>()
    .insert_resource(Input(2))
    .init_resource::<Observed>()
    .register_generator(
        "identity",
        FunctionSignature::new()
            .required("value", ValueType::Any)
            .emits(ValueType::Any),
        |_, args| Ok(vec![args.take::<Value>("value")?]),
    )
    .register_generator(
        "reward",
        FunctionSignature::new()
            .required("amount", ValueType::Integer)
            .emits(ValueType::data::<Reward>()),
        |_, args| {
            Ok(vec![Value::data(Reward {
                amount: args.take("amount")?,
            })])
        },
    )
    .register_contextual::<Res<Input>, _>(
        "read",
        FunctionSignature::new()
            .required("value", ValueType::Integer)
            .emits(ValueType::Integer),
        |_, args, input| Ok(vec![(args.take::<i64>("value")? * input.0).into()]),
    )
    .add_systems(PostUpdate, observe.after(LootSet::Publish));
    app
}

fn catalog(app: &App, source: &str) -> Arc<Catalog> {
    app.world()
        .resource::<LootRegistry>()
        .compile([SourceDocument::new("test.loot", source)])
        .unwrap()
}
fn pump(app: &mut App, count: usize) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        app.update();
        if app.world().resource::<Observed>().0.len() >= count {
            return;
        }
        std::thread::yield_now();
    }
    panic!("expected {count} outcomes");
}

fn gameplay_count(app: &mut App) -> usize {
    let mut query = app.world_mut().query_filtered::<Entity, With<Health>>();
    query.iter(app.world()).count()
}

#[test]
fn pure_generators_finish_in_one_update_and_return_consumer_defined_data() {
    let mut app = fixture();
    let owner = app.world_mut().spawn(Health(100)).id();
    let catalog = catalog(&app, "table \"root\"\n  @reward amount=$amount as=reward\n");
    app.world_mut().write_message(
        LootRequest::<Value>::from_catalog(catalog, "root", 4)
            .input("amount", 21i64)
            .owner(owner),
    );
    app.update();
    let observed = app.world().resource::<Observed>();
    assert_eq!(observed.0.len(), 1);
    assert_eq!(observed.0[0].owner, Some(owner));
    let generated = observed.0[0].result.as_ref().unwrap();
    assert_eq!(
        generated
            .iter()
            .filter_map(Value::get_data::<Reward>)
            .collect::<Vec<_>>(),
        [&Reward { amount: 21 }]
    );
    assert_eq!(app.world().get::<Health>(owner), Some(&Health(100)));
    assert_eq!(gameplay_count(&mut app), 1);
}

#[test]
fn native_queries_are_read_only_and_the_alias_stays_in_the_bag() {
    let mut app = fixture();
    let target = app.world_mut().spawn(Health(37)).id();
    app.register_contextual::<Query<&Health>, _>(
        "health",
        FunctionSignature::new()
            .required("target", ValueType::reference("entity"))
            .emits(ValueType::Integer),
        |_, args, health| {
            let target: Reference = args.take("target")?;
            let health = health
                .get(Entity::from_bits(target.id))
                .map_err(|_| "target missing")?;
            Ok(vec![health.0.into()])
        },
    );
    let catalog = catalog(
        &app,
        "table \"root\"\n  @health target=$target as=health\n  @identity value=$health\n",
    );
    app.world_mut().write_message(
        LootRequest::<Value>::from_catalog(catalog, "root", 0)
            .input("target", Reference::new("entity", target.to_bits())),
    );
    pump(&mut app, 1);
    assert_eq!(
        app.world().resource::<Observed>().0[0]
            .result
            .clone()
            .unwrap(),
        [37i64.into(), 37i64.into()]
    );
    assert_eq!(app.world().get::<Health>(target), Some(&Health(37)));
    assert_eq!(gameplay_count(&mut app), 1);
}

#[test]
fn contextual_generators_suspend_only_their_own_roll() {
    let mut app = fixture();
    let catalog = catalog(
        &app,
        "table \"context\"\n  @read value=5\ntable \"pure\"\n  @identity value=7\n",
    );
    app.world_mut()
        .write_message(LootRequest::<Value>::from_catalog(
            catalog.clone(),
            "context",
            0,
        ));
    let request = LootRequest::<Value>::from_catalog(catalog, "pure", 0);
    let pure_id = request.id();
    app.world_mut().write_message(request);
    app.update();
    let observed = app.world().resource::<Observed>();
    assert_eq!(observed.0.len(), 1);
    assert_eq!(observed.0[0].id, pure_id);
    assert_eq!(observed.0[0].result.clone().unwrap(), [7i64.into()]);
    pump(&mut app, 2);
    assert_eq!(
        app.world().resource::<Observed>().0[1]
            .result
            .clone()
            .unwrap(),
        [10i64.into()]
    );
}

#[test]
fn request_ids_are_available_before_submission_and_survive_contextual_generation() {
    let mut app = fixture();
    let target = app.world_mut().spawn(Health(100)).id();
    let catalog = catalog(
        &app,
        "table \"root\"\n  @reward amount=10\n  @read value=3\n",
    );
    let request = LootRequest::<Value>::from_catalog(catalog, "root", 0).owner(target);
    let id = request.id();
    assert_ne!(id, LootRequest::<Value>::new("root").id());
    app.world_mut().write_message(request);
    app.update();
    assert!(app.world().resource::<Observed>().0.is_empty());
    pump(&mut app, 1);
    let finished = &app.world().resource::<Observed>().0[0];
    assert_eq!(finished.id, id);
    assert_eq!(finished.owner, Some(target));
    assert_eq!(finished.result.as_ref().unwrap().len(), 2);
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(app.world().resource::<Observed>().0.len(), 1);
    assert_eq!(app.world().get::<Health>(target), Some(&Health(100)));
    assert_eq!(app.world().resource::<Input>().0, 2);
}

#[test]
fn errors_publish_no_partial_bag_and_leave_gameplay_state_unchanged() {
    let mut app = fixture();
    let target = app.world_mut().spawn(Health(100)).id();
    app.register_contextual::<Res<Input>, _>("reject", FunctionSignature::new(), |_, _, _| {
        Err("ineligible context".into())
    });
    let catalog = catalog(
        &app,
        "table \"root\"\n  @reward amount=10\n  @reject\n  @reward amount=20\n",
    );
    app.world_mut()
        .write_message(LootRequest::<Value>::from_catalog(catalog, "root", 0));
    pump(&mut app, 1);
    let error = app.world().resource::<Observed>().0[0]
        .result
        .as_ref()
        .unwrap_err();
    assert!(error.message.contains("ineligible context"));
    assert_eq!(app.world().get::<Health>(target), Some(&Health(100)));
    assert_eq!(app.world().resource::<Input>().0, 2);
    assert_eq!(gameplay_count(&mut app), 1);
}

#[test]
fn deferred_world_commands_are_rejected_even_though_bevy_calls_them_read_only() {
    let mut app = fixture();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.register_contextual::<Commands, _>("bad", FunctionSignature::new(), |_, _, _| {
            Ok(Vec::new())
        });
    }));
    let panic = result.unwrap_err();
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap();
    assert!(message.contains("cannot use deferred world mutations"));
    assert!(
        app.world()
            .resource::<LootRegistry>()
            .signatures()
            .get("bad")
            .is_none()
    );
    assert_eq!(gameplay_count(&mut app), 0);
}

#[test]
fn deterministic_streams_do_not_depend_on_extra_updates_or_other_requests() {
    fn run(extra_updates: bool) -> Vec<Vec<Value>> {
        let mut app = fixture();
        let catalog = catalog(
            &app,
            "table \"root\"\n  @read value=1..100 as=sample\n  @identity value=$sample\n  choose\n    @reward amount=1..100\n    @reward amount=101..200\n",
        );
        for seed in 0..16 {
            app.world_mut()
                .write_message(LootRequest::<Value>::from_catalog(
                    catalog.clone(),
                    "root",
                    seed,
                ));
            if extra_updates {
                app.update();
            }
        }
        pump(&mut app, 16);
        let mut finished = app.world().resource::<Observed>().0.clone();
        finished.sort_by_key(|finished| finished.id.0);
        finished
            .into_iter()
            .map(|finished| finished.result.unwrap())
            .collect()
    }
    assert_eq!(run(false), run(true));
}

#[test]
fn argument_consumption_and_native_output_schemas_are_checked() {
    let mut app = fixture();
    app.register_generator(
        "unused",
        FunctionSignature::new().required("value", ValueType::Integer),
        |_, _| Ok(Vec::new()),
    );
    app.register_generator(
        "wrong",
        FunctionSignature::new().emits(ValueType::String),
        |_, _| Ok(vec![1i64.into()]),
    );
    let catalog = catalog(
        &app,
        "table \"unused\"\n  @unused value=1\ntable \"wrong\"\n  @wrong\n",
    );
    for table in ["unused", "wrong"] {
        app.world_mut()
            .write_message(LootRequest::<Value>::from_catalog(
                catalog.clone(),
                table,
                0,
            ));
    }
    pump(&mut app, 2);
    let observed = app.world().resource::<Observed>();
    assert!(
        observed.0[0]
            .result
            .as_ref()
            .unwrap_err()
            .message
            .contains("not consumed")
    );
    assert!(
        observed.0[1]
            .result
            .as_ref()
            .unwrap_err()
            .message
            .contains("generator emitted")
    );
}

#[test]
fn required_inputs_and_foreign_protocols_are_checked_before_generation() {
    let mut app = fixture();
    let own = catalog(&app, "table \"root\"\n  @identity value=$input\n");
    app.world_mut()
        .write_message(LootRequest::<Value>::from_catalog(own.clone(), "root", 0));
    app.world_mut()
        .write_message(LootRequest::<Value>::from_catalog(own, "missing", 0));
    let mut signatures = FunctionSignatures::new();
    signatures
        .register(
            "identity",
            FunctionSignature::new()
                .required("value", ValueType::Integer)
                .emits(ValueType::Integer),
        )
        .unwrap();
    let foreign = Arc::new(
        Catalog::compile(
            [SourceDocument::new(
                "foreign.loot",
                "table \"root\"\n  @identity value=1\n",
            )],
            &signatures,
        )
        .unwrap(),
    );
    app.world_mut()
        .write_message(LootRequest::<Value>::from_catalog(foreign, "root", 0));
    pump(&mut app, 3);
    let messages: Vec<_> = app
        .world()
        .resource::<Observed>()
        .0
        .iter()
        .map(|finished| finished.result.as_ref().unwrap_err().message.as_str())
        .collect();
    assert!(messages[0].contains("requires input"));
    assert!(messages[1].contains("unknown table"));
    assert!(messages[2].contains("catalog's signature"));
}

#[test]
fn asset_loader_links_generators_and_preserves_locations() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("reward.loot"),
        "table \"root\"\n  @reward amount=8\n",
    )
    .unwrap();
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            file_path: directory.path().to_string_lossy().into_owned(),
            ..Default::default()
        },
        LootPlugin::default().with_sources(["reward.loot"]),
    ))
    .add_loot_output::<Value>()
    .init_resource::<Observed>()
    .register_generator(
        "reward",
        FunctionSignature::new()
            .required("amount", ValueType::Integer)
            .emits(ValueType::data::<Reward>()),
        |_, args| {
            Ok(vec![Value::data(Reward {
                amount: args.take("amount")?,
            })])
        },
    )
    .add_systems(PostUpdate, observe.after(LootSet::Publish));
    app.finish();
    app.cleanup();
    app.world_mut()
        .write_message(LootRequest::<Value>::new("root").seed(0));
    pump(&mut app, 1);
    let observed = app.world().resource::<Observed>();
    assert_eq!(
        observed.0[0]
            .result
            .as_ref()
            .unwrap()
            .iter()
            .filter_map(Value::get_data::<Reward>)
            .collect::<Vec<_>>(),
        [&Reward { amount: 8 }]
    );
    let inslag_loot::TraceEvent::Called {
        location,
        function,
        seed,
    } = &observed.0[0].trace[0]
    else {
        panic!()
    };
    assert!(location.source.ends_with("reward.loot"));
    assert_eq!(location.line, 2);
    assert_eq!(function, "reward");
    assert_eq!(*seed, fastrand::Rng::with_seed(0).u64(..));
}

#[test]
fn active_contextual_rolls_keep_their_original_catalog_when_sources_change() {
    let mut app = fixture();
    let old = catalog(
        &app,
        "table \"root\"\n  @read value=1\n  @identity value=42\n",
    );
    *app.world_mut().resource_mut::<Sources>() = Sources {
        handles: Vec::new(),
        prepared: PreparedLoot::Ready(old),
        dirty: false,
        compiled: true,
    };
    app.world_mut()
        .write_message(LootRequest::<Value>::new("root").seed(0));
    app.update();
    let new = catalog(&app, "table \"root\"\n  @identity value=99\n");
    app.world_mut().resource_mut::<Sources>().prepared = PreparedLoot::Ready(new);
    app.world_mut()
        .write_message(LootRequest::<Value>::new("root").seed(0));
    pump(&mut app, 2);
    let bags: Vec<_> = app
        .world()
        .resource::<Observed>()
        .0
        .iter()
        .map(|finished| finished.result.clone().unwrap())
        .collect();
    assert!(bags.contains(&vec![2i64.into(), 42i64.into()]));
    assert!(bags.contains(&vec![99i64.into()]));
}

#[test]
fn direct_pure_generation_refuses_to_query_the_world() {
    let app = fixture();
    let catalog = catalog(&app, "table \"root\"\n  @read value=1\n");
    assert!(
        app.world()
            .resource::<LootRegistry>()
            .generate_values(catalog, "root", Default::default(), 0)
            .unwrap_err()
            .message
            .contains("read-only Bevy context")
    );
    assert_eq!(app.world().resource::<Input>().0, 2);
}

#[test]
fn failed_preparation_reports_an_error_without_generating_anything() {
    let mut app = fixture();
    *app.world_mut().resource_mut::<Sources>() = Sources {
        handles: Vec::new(),
        prepared: PreparedLoot::Failed(LootError::request("source unavailable")),
        dirty: false,
        compiled: false,
    };
    app.world_mut()
        .write_message(LootRequest::<Value>::new("root"));
    pump(&mut app, 1);
    assert!(
        app.world().resource::<Observed>().0[0]
            .result
            .as_ref()
            .unwrap_err()
            .message
            .contains("unavailable")
    );
    assert_eq!(gameplay_count(&mut app), 0);
}

#[test]
fn queued_requests_outlive_bevy_message_retention_without_being_reissued() {
    let mut app = fixture();
    let catalog = catalog(&app, "table \"root\"\n  @identity value=$value\n");
    *app.world_mut().resource_mut::<Sources>() = Sources {
        handles: Vec::new(),
        prepared: PreparedLoot::Loading,
        dirty: false,
        compiled: false,
    };
    let mut expected = HashMap::new();
    for value in 0..64i64 {
        let request = LootRequest::<Value>::new("root").input("value", value);
        expected.insert(request.id(), value);
        app.world_mut().write_message(request);
    }
    for _ in 0..8 {
        app.update();
        assert!(app.world().resource::<Observed>().0.is_empty());
        assert_eq!(app.world().resource::<Runtime>().pending.len(), 64);
    }
    app.world_mut().resource_mut::<Sources>().prepared = PreparedLoot::Ready(catalog);
    pump(&mut app, 64);
    for finished in &app.world().resource::<Observed>().0 {
        let value = expected
            .remove(&finished.id)
            .expect("one result per request");
        assert_eq!(finished.result.as_ref().unwrap(), &[Value::Integer(value)]);
    }
    assert!(expected.is_empty());
    assert!(app.world().resource::<Runtime>().pending.is_empty());
    for _ in 0..8 {
        app.update();
    }
    assert_eq!(app.world().resource::<Observed>().0.len(), 64);
}

#[test]
fn queued_requests_all_finish_if_preparation_fails_after_loading() {
    let mut app = fixture();
    *app.world_mut().resource_mut::<Sources>() = Sources {
        handles: Vec::new(),
        prepared: PreparedLoot::Loading,
        dirty: false,
        compiled: false,
    };
    for _ in 0..4 {
        app.world_mut()
            .write_message(LootRequest::<Value>::new("root"));
    }
    for _ in 0..8 {
        app.update();
    }
    app.world_mut().resource_mut::<Sources>().prepared =
        PreparedLoot::Failed(LootError::request("invalid source"));
    pump(&mut app, 4);
    assert!(app.world().resource::<Observed>().0.iter().all(|finished| {
        finished
            .result
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("invalid source")
    }));
    assert!(app.world().resource::<Runtime>().pending.is_empty());
}

#[test]
fn source_revisions_recompile_once_and_recover_from_invalid_or_removed_assets() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("reward.loot"),
        "table \"root\"\n  1\n",
    )
    .unwrap();
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            file_path: directory.path().to_string_lossy().into_owned(),
            ..Default::default()
        },
        LootPlugin::default().with_sources(["reward.loot"]),
    ))
    .add_loot_output::<Value>()
    .init_resource::<Observed>()
    .add_systems(PostUpdate, observe.after(LootSet::Publish));
    app.world_mut()
        .write_message(LootRequest::<Value>::new("root"));
    pump(&mut app, 1);
    let handle = app.world().resource::<Sources>().handles[0].clone();

    fn prepared(app: &App) -> Arc<Catalog> {
        match &app.world().resource::<Sources>().prepared {
            PreparedLoot::Ready(catalog) => catalog.clone(),
            _ => panic!("sources are not ready"),
        }
    }

    let initial = prepared(&app);
    for _ in 0..8 {
        app.update();
        assert!(Arc::ptr_eq(&initial, &prepared(&app)));
    }
    app.world_mut()
        .resource_mut::<Assets<LootSource>>()
        .get_mut(&handle)
        .unwrap()
        .text = "table \"root\"\n  2\n".into();
    for _ in 0..4 {
        app.update();
    }
    let revised = prepared(&app);
    assert!(!Arc::ptr_eq(&initial, &revised));
    app.world_mut()
        .write_message(LootRequest::<Value>::new("root"));
    pump(&mut app, 2);
    assert_eq!(
        app.world().resource::<Observed>().0[1]
            .result
            .as_ref()
            .unwrap(),
        &[Value::Integer(2)]
    );
    for _ in 0..8 {
        app.update();
        assert!(Arc::ptr_eq(&revised, &prepared(&app)));
    }

    app.world_mut()
        .resource_mut::<Assets<LootSource>>()
        .get_mut(&handle)
        .unwrap()
        .text = "invalid".into();
    for _ in 0..4 {
        app.update();
    }
    app.world_mut()
        .write_message(LootRequest::<Value>::new("root"));
    pump(&mut app, 3);
    assert!(app.world().resource::<Observed>().0[2].result.is_err());

    app.world_mut()
        .resource_mut::<Assets<LootSource>>()
        .remove(handle.id());
    for _ in 0..4 {
        app.update();
    }
    app.world_mut()
        .write_message(LootRequest::<Value>::new("root"));
    pump(&mut app, 4);
    assert!(
        app.world().resource::<Observed>().0[3]
            .result
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("unavailable")
    );

    app.world_mut()
        .resource_mut::<Assets<LootSource>>()
        .insert(
            handle.id(),
            LootSource {
                text: "table \"root\"\n  3\n".into(),
            },
        )
        .unwrap();
    for _ in 0..4 {
        app.update();
    }
    app.world_mut()
        .write_message(LootRequest::<Value>::new("root"));
    pump(&mut app, 5);
    assert_eq!(
        app.world().resource::<Observed>().0[4]
            .result
            .as_ref()
            .unwrap(),
        &[Value::Integer(3)]
    );
}

#[test]
fn output_validation_is_cached_by_root_and_output_type_including_failures() {
    let app = fixture();
    let catalog = catalog(
        &app,
        "table \"root\"\n  #child\ntable \"child\"\n  1\ntable \"text\"\n  text\n",
    );
    let mut cache = OutputValidationCache::default();
    let checks = std::cell::Cell::new(0);
    let mut errors = Vec::new();
    for _ in 0..8 {
        for table in ["root", "text"] {
            let integer = cache.validate(&catalog, table, TypeId::of::<i64>(), |catalog, table| {
                checks.set(checks.get() + 1);
                catalog.validate_output::<i64>(table)
            });
            let text = cache.validate(&catalog, table, TypeId::of::<String>(), |catalog, table| {
                checks.set(checks.get() + 1);
                catalog.validate_output::<String>(table)
            });
            if table == "root" {
                integer.unwrap();
                errors.push(text.unwrap_err());
            } else {
                assert!(integer.is_err());
                text.unwrap();
            }
        }
    }
    assert_eq!(checks.get(), 4);
    assert!(errors.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(errors[0].location.line, 4);
}

#[test]
fn output_validation_cache_does_not_retain_catalogs_or_unknown_request_names() {
    let app = fixture();
    let catalog = catalog(&app, "table \"root\"\n  1\n");
    let mut cache = OutputValidationCache::default();
    cache
        .validate(
            &catalog,
            "root",
            TypeId::of::<i64>(),
            Catalog::validate_output::<i64>,
        )
        .unwrap();
    for index in 0..100 {
        assert!(
            cache
                .validate(
                    &catalog,
                    &format!("missing{index}"),
                    TypeId::of::<i64>(),
                    Catalog::validate_output::<i64>,
                )
                .is_err()
        );
    }
    assert_eq!(cache.outputs[&TypeId::of::<i64>()].len(), 1);
    drop(catalog);
    assert!(cache.catalog.upgrade().is_none());
}

#[test]
fn requests_reuse_output_validation_across_batches_and_updates() {
    use std::sync::atomic::AtomicUsize;

    static CHECKS: AtomicUsize = AtomicUsize::new(0);

    struct Number(i64);

    impl LootOutput for Number {
        fn accepts_type(value_type: &ValueType) -> bool {
            CHECKS.fetch_add(1, Ordering::SeqCst);
            i64::accepts_type(value_type)
        }

        fn accepts_value(value: &Value) -> bool {
            CHECKS.fetch_add(1, Ordering::SeqCst);
            i64::accepts_value(value)
        }

        fn from_value(value: Value) -> Result<Self, String> {
            i64::from_value(value).map(Self)
        }
    }

    let mut app = fixture();
    app.add_loot_output::<Number>();
    let catalog = catalog(&app, "table \"root\"\n  #child\ntable \"child\"\n  7\n");
    let mut cursor = app
        .world()
        .resource::<Messages<LootFinished<Number>>>()
        .get_cursor_current();
    for _ in 0..3 {
        for _ in 0..8 {
            app.world_mut()
                .write_message(LootRequest::<Number>::from_catalog(
                    catalog.clone(),
                    "root",
                    0,
                ));
        }
        app.update();
        let mut count = 0;
        for finished in cursor.read(app.world().resource::<Messages<LootFinished<Number>>>()) {
            let result = finished.result.as_ref().unwrap();
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].0, 7);
            count += 1;
        }
        assert_eq!(count, 8);
        assert_eq!(CHECKS.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn output_validation_is_invalidated_when_assets_change_and_recovers_after_failures() {
    let directory = tempfile::tempdir().unwrap();
    let source = "table \"root\"\n  @number\n  #child\ntable \"child\"\n  1\n";
    std::fs::write(directory.path().join("reward.loot"), source).unwrap();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = calls.clone();
    let mut app = App::new();
    app.add_plugins((
        TaskPoolPlugin::default(),
        AssetPlugin {
            file_path: directory.path().to_string_lossy().into_owned(),
            ..Default::default()
        },
        LootPlugin::default().with_sources(["reward.loot"]),
    ))
    .add_loot_output::<i64>()
    .add_loot_function("number", move || {
        observed.fetch_add(1, Ordering::SeqCst);
        7i64
    });

    fn roll(app: &mut App) -> Result<Vec<i64>, LootError> {
        let mut cursor = app
            .world()
            .resource::<Messages<LootFinished<i64>>>()
            .get_cursor_current();
        app.world_mut()
            .write_message(LootRequest::<i64>::new("root"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            assert!(std::time::Instant::now() < deadline);
            app.update();
            if let Some(finished) = cursor
                .read(app.world().resource::<Messages<LootFinished<i64>>>())
                .next()
            {
                return finished.result.clone();
            }
            std::thread::yield_now();
        }
    }

    assert_eq!(roll(&mut app).unwrap(), [7, 1]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let handle = app.world().resource::<Sources>().handles[0].clone();
    for (child, expected_calls) in [("text", 1), ("2", 2), ("false", 2), ("3", 3)] {
        app.world_mut()
            .resource_mut::<Assets<LootSource>>()
            .get_mut(&handle)
            .unwrap()
            .text = source.replace("  1\n", &format!("  {child}\n"));
        for _ in 0..4 {
            app.update();
        }
        if let Ok(value) = child.parse::<i64>() {
            assert_eq!(roll(&mut app).unwrap(), [7, value]);
        } else {
            let error = roll(&mut app).unwrap_err();
            assert_eq!(error.location.line, 5);
            assert!(error.message.contains("not supported"));
            assert_eq!(roll(&mut app).unwrap_err(), error);
        }
        assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
    }
}
