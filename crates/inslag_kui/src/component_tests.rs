use crate::{runtime::test_support, *};
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use std::collections::BTreeMap;

const VIEW: &str = r#"<view xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="test">"#;
const COMPONENT: &str = r#"<component xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="test">"#;

#[test]
fn component_instance_attributes_escape_xml_exactly_once() {
    let source = format!(
        r#"{VIEW}<use component="box" value="A &amp; B" bind:visible="phase == 'ready' and count &gt; 0"/></view>"#
    );
    let definition = parse("instance.kui", &source).unwrap();
    let instance = &definition.children[0];
    assert_eq!(instance.attributes["value"], "A & B");
    assert!(
        matches!(&instance.bindings[0], KuiBinding::Visible(expression) if expression == "phase == 'ready' and count > 0")
    );
}

#[test]
fn builtin_aliases_and_root_data_are_rejected_but_nested_fields_work() {
    let invalid = format!(
        r#"{VIEW}<repeat bind:items="items" as="math" key="id"><text bind:value="math.id"/></repeat></view>"#
    );
    let error = parse("collision.kui", &invalid).unwrap_err().to_string();
    assert!(
        error.contains("math") && error.contains("reserved"),
        "{error}"
    );

    let source = format!(r#"{VIEW}<text bind:text="printer.type"/></view>"#);
    let mut app = test_support::app(&[("main.kui", &source)]);
    let valid = mount(
        &mut app,
        BTreeMap::from([(
            "printer".into(),
            KuiValue::Record(BTreeMap::from([(
                "type".into(),
                KuiValue::String("laser".into()),
            )])),
        )]),
    );
    assert_eq!(
        app.world().get::<KuiView>(valid).unwrap().status(),
        &KuiViewStatus::Ready
    );
    assert!(texts(&mut app).contains(&"laser".to_string()));
    let invalid = mount(&mut app, BTreeMap::from([("math".into(), KuiValue::Nil)]));
    assert!(
        matches!(app.world().get::<KuiView>(invalid).unwrap().status(), KuiViewStatus::Failed(error) if error.contains("math") && error.contains("reserved"))
    );
}

fn mount(app: &mut App, data: BTreeMap<String, KuiValue>) -> Entity {
    let owner = app.world_mut().spawn_empty().id();
    let handle = app.world().resource::<AssetServer>().load("main.kui");
    let mut view = KuiView::new(owner, handle);
    view.data = data;
    let mount = app.world_mut().spawn(view).id();
    crate::tests::wait_for(app, |app| {
        app.world().get::<KuiView>(mount).unwrap().status() != &KuiViewStatus::Loading
    });
    mount
}

fn texts(app: &mut App) -> Vec<String> {
    app.world_mut()
        .query::<&Text>()
        .iter(app.world())
        .map(|t| t.0.clone())
        .collect()
}

fn row(id: i64, label: &str) -> KuiValue {
    KuiValue::Record(BTreeMap::from([
        ("id".into(), KuiValue::Integer(id)),
        ("label".into(), KuiValue::String(label.into())),
    ]))
}

#[test]
fn idle_binding_caches_preserve_input_ticks_and_nested_edits_still_update() {
    let source = format!(r#"{VIEW}<text bind:text="model.label"/></view>"#);
    let mut app = test_support::app(&[("main.kui", &source)]);
    let mount = mount(
        &mut app,
        BTreeMap::from([(
            "model".into(),
            KuiValue::Record(BTreeMap::from([(
                "label".into(),
                KuiValue::String("First".into()),
            )])),
        )]),
    );
    let tick = app
        .world()
        .entity(mount)
        .get_ref::<KuiView>()
        .unwrap()
        .last_changed();
    for _ in 0..4 {
        app.update();
        assert_eq!(
            app.world()
                .entity(mount)
                .get_ref::<KuiView>()
                .unwrap()
                .last_changed(),
            tick,
            "internal binding work must not invalidate the view"
        );
    }
    let mut view = app.world_mut().get_mut::<KuiView>(mount).unwrap();
    let KuiValue::Record(model) = view.data.get_mut("model").unwrap() else {
        unreachable!()
    };
    *model.get_mut("label").unwrap() = KuiValue::String("Updated".into());
    app.update();
    assert_eq!(texts(&mut app), ["Updated"]);
}

#[test]
fn repeat_slots_refresh_external_callers_without_row_changes() {
    let source = format!(
        r#"{VIEW}<import name="list" src="list.kui"/><use component="list"><prop name="items" bind:value="items"/><fill slot="detail"><text bind:text="title"/></fill></use></view>"#
    );
    let component = format!(
        r#"{COMPONENT}<prop name="items" type="sequence"/><node><repeat bind:items="props.items" as="item" key="id"><node><slot name="detail"/></node></repeat></node></component>"#
    );
    let mut app = test_support::app(&[("main.kui", &source), ("list.kui", &component)]);
    let mount = mount(
        &mut app,
        BTreeMap::from([
            ("title".into(), KuiValue::String("First".into())),
            (
                "items".into(),
                KuiValue::Sequence(vec![row(1, "A"), row(2, "B")]),
            ),
        ]),
    );
    assert_eq!(texts(&mut app), ["First", "First"]);
    let retained = app
        .world_mut()
        .query_filtered::<Entity, With<KuiRepeatItem>>()
        .iter(app.world())
        .collect::<Vec<_>>();
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("title".into(), KuiValue::String("Updated".into()));
    app.update();
    assert_eq!(texts(&mut app), ["Updated", "Updated"]);
    assert!(
        retained
            .into_iter()
            .all(|entity| app.world().entities().contains(entity))
    );
}

#[test]
fn settled_views_receive_late_failures_and_detect_already_failed_replacements() {
    for late_event in [true, false] {
        let source = format!(r#"{VIEW}<node/></view>"#);
        let mut app = test_support::app(&[("main.kui", &source)]);
        let mount = mount(&mut app, BTreeMap::new());
        let root = app.world().get::<KuiView>(mount).unwrap().roots()[0];
        for _ in 0..4 {
            app.update();
        }
        if late_event {
            let id = app.world().get::<KuiView>(mount).unwrap().asset.id();
            app.world_mut()
                .write_message(bevy::asset::AssetLoadFailedEvent::<KuiAsset> {
                    id,
                    path: "main.kui".into(),
                    error: bevy::asset::AssetLoadError::AssetMetaReadError,
                });
        } else {
            let missing = app
                .world()
                .resource::<AssetServer>()
                .load::<KuiAsset>("missing.kui");
            crate::tests::wait_for(&mut app, |app| {
                matches!(
                    app.world()
                        .resource::<AssetServer>()
                        .get_load_state(missing.id()),
                    Some(bevy::asset::LoadState::Failed(_))
                )
            });
            for _ in 0..4 {
                app.update(); // Drain the failure message before changing the handle.
            }
            app.world_mut().get_mut::<KuiView>(mount).unwrap().asset = missing;
        }
        app.update();
        assert!(matches!(
            app.world().get::<KuiView>(mount).unwrap().status(),
            KuiViewStatus::Failed(_)
        ));
        assert!(!app.world().entities().contains(root));
    }
}

#[test]
fn lookup_props_and_internal_keys_keep_caller_and_component_scopes_separate() {
    let main = format!(
        r#"{VIEW}<import name="value" src="value.kui"/><node><repeat bind:items="items" as="item" key="id"><use component="value"><prop name="values" bind:value="values"/><prop name="key" bind:value="item.label"/><prop name="number" bind:value="values[item.label]"/><fill slot="detail"><text template="Caller {{values[item.label]}}"/></fill></use></repeat></node></view>"#
    );
    let component = format!(
        r#"{COMPONENT}<prop name="values" type="record"/><prop name="key" type="string"/><prop name="number" type="number"/><node><text template="{{props.values[props.key]}} / {{props.number}}"/><slot name="detail"/></node></component>"#
    );
    let mut app = test_support::app(&[("main.kui", &main), ("value.kui", &component)]);
    let mount = mount(
        &mut app,
        BTreeMap::from([
            ("items".into(), KuiValue::Sequence(vec![row(1, "a")])),
            (
                "values".into(),
                KuiValue::Record(BTreeMap::from([
                    ("a".into(), KuiValue::Integer(7)),
                    ("b".into(), KuiValue::Integer(9)),
                ])),
            ),
        ]),
    );
    let mut rendered = texts(&mut app);
    rendered.sort();
    assert_eq!(rendered, ["7 / 7", "Caller 7"]);
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("items".into(), KuiValue::Sequence(vec![row(1, "b")]));
    app.update();
    let mut rendered = texts(&mut app);
    rendered.sort();
    assert_eq!(rendered, ["9 / 9", "Caller 9"]);
}

#[test]
fn component_contract_checks_paths_used_as_dynamic_keys() {
    for path in [
        "props.values[owner.key]",
        "props.values[props.missing]",
        "props.values[props.key.field]",
    ] {
        let source = format!(
            r#"{COMPONENT}<prop name="values" type="record"/><prop name="key" type="string"/><text bind:text="{path}"/></component>"#
        );
        let error = parse("invalid-key.kui", &source).unwrap_err();
        assert!(
            error.to_string().contains(if path.contains("owner") {
                "must use declared props"
            } else if path.contains("missing") {
                "undeclared prop"
            } else {
                "scalar prop"
            }),
            "{error}"
        );
    }
}

#[test]
fn keyed_components_preserve_identity_defaults_and_caller_slot_scope() {
    let main = format!(
        r#"{VIEW}<import name="card" src="card.kui"/><node><repeat bind:items="items" as="item" key="id"><use component="card"><prop name="label" bind:value="item.label"/><fill slot="detail"><text template="{{title}} · {{item.label}}"/></fill></use></repeat></node></view>"#
    );
    let card = format!(
        r#"{COMPONENT}<prop name="label" type="string"/><prop name="suffix" type="string" default="default"/><node><text id="label" template="{{props.label}} {{props.suffix}}"/><slot name="detail"><text>Fallback</text></slot></node></component>"#
    );
    let mut app = test_support::app(&[("main.kui", &main), ("card.kui", &card)]);
    let mount = mount(
        &mut app,
        BTreeMap::from([
            ("title".into(), KuiValue::String("Caller".into())),
            (
                "items".into(),
                KuiValue::Sequence(vec![row(1, "A"), row(2, "B")]),
            ),
        ]),
    );
    assert_eq!(
        app.world().get::<KuiView>(mount).unwrap().status(),
        &KuiViewStatus::Ready
    );
    let mut rendered = texts(&mut app);
    rendered.sort();
    assert_eq!(
        rendered,
        ["A default", "B default", "Caller · A", "Caller · B"]
    );
    let instances: BTreeMap<_, _> = app
        .world_mut()
        .query::<(Entity, &KuiRepeatItem, &KuiComponentInstance)>()
        .iter(app.world())
        .map(|(entity, row, component)| {
            assert_eq!(component.source, "card.kui");
            (row.key.clone(), entity)
        })
        .collect();
    assert_eq!(instances.len(), 2);
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert(
            "items".into(),
            KuiValue::Sequence(vec![row(2, "Changed"), row(1, "A")]),
        );
    app.update();
    let next: BTreeMap<_, _> = app
        .world_mut()
        .query::<(Entity, &KuiRepeatItem)>()
        .iter(app.world())
        .map(|(entity, row)| (row.key.clone(), entity))
        .collect();
    assert_eq!(instances, next);
    let all = texts(&mut app);
    assert!(all.contains(&"Changed default".into()));
    assert!(all.contains(&"Caller · Changed".into()));
    let root = app.world().get::<KuiView>(mount).unwrap().roots()[0];
    assert_eq!(
        app.world().get::<Children>(root).unwrap().to_vec(),
        [
            instances[&KuiItemKey::Integer(2)],
            instances[&KuiItemKey::Integer(1)]
        ]
    );
}

#[test]
fn nested_props_recover_without_owner_fallback_or_entity_replacement() {
    let main = format!(
        r#"{VIEW}<import name="outer" src="outer.kui"/><use component="outer"><prop name="label" bind:value="label"/></use></view>"#
    );
    let outer = format!(
        r#"{COMPONENT}<import name="inner" src="inner.kui"/><prop name="label" type="string"/><node><use component="inner"><prop name="label" bind:value="props.label"/></use></node></component>"#
    );
    let inner = format!(
        r#"{COMPONENT}<prop name="label" type="string"/><text bind:text="props.label">fallback</text></component>"#
    );
    let mut app = test_support::app(&[
        ("main.kui", &main),
        ("outer.kui", &outer),
        ("inner.kui", &inner),
    ]);
    let mount = mount(
        &mut app,
        BTreeMap::from([("label".into(), KuiValue::String("First".into()))]),
    );
    assert_eq!(texts(&mut app), ["First"]);
    let text = app
        .world_mut()
        .query_filtered::<Entity, With<Text>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("label".into(), KuiValue::Integer(3));
    app.update();
    assert_eq!(app.world().get::<Text>(text).unwrap().0, "First");
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("label".into(), KuiValue::String("Recovered".into()));
    app.update();
    assert_eq!(app.world().get::<Text>(text).unwrap().0, "Recovered");
}

#[test]
fn loader_reports_contract_errors_cycles_and_nested_buttons() {
    let simple = format!(
        r#"{COMPONENT}<prop name="amount" type="number"/><node><slot name="body"/></node></component>"#
    );
    for (body, expected) in [
        (r#"<use component="part"/>"#, "missing required prop"),
        (
            r#"<use component="part"><prop name="amount" value="bad"/></use>"#,
            "valid Number literal",
        ),
        (
            r#"<use component="part"><prop name="amount" value="1"/><prop name="unknown" value="x"/></use>"#,
            "unknown prop",
        ),
        (
            r#"<use component="part"><prop name="amount" value="1"/><fill slot="unknown"><node/></fill></use>"#,
            "unknown slot",
        ),
        (
            r#"<use component="part"><prop name="amount" value="1"/><on name="unknown" action="go"/></use>"#,
            "unknown event",
        ),
    ] {
        let source = format!(r#"{VIEW}<import name="part" src="part.kui"/>{body}</view>"#);
        let mut app = test_support::app(&[("main.kui", &source), ("part.kui", &simple)]);
        let mount = mount(&mut app, BTreeMap::new());
        let KuiViewStatus::Failed(message) = app.world().get::<KuiView>(mount).unwrap().status()
        else {
            panic!("expected failure: {body}")
        };
        assert!(message.contains(expected), "{message}");
        assert!(
            app.world()
                .get::<KuiView>(mount)
                .unwrap()
                .roots()
                .is_empty()
        );
    }
    let main =
        format!(r#"{VIEW}<import name="part" src="part.kui"/><use component="part"/></view>"#);
    let cycle = format!(
        r#"{COMPONENT}<import name="self" src="part.kui"/><use component="self"/></component>"#
    );
    let mut app = test_support::app(&[("main.kui", &main), ("part.kui", &cycle)]);
    let cycle_mount = mount(&mut app, BTreeMap::new());
    assert!(
        matches!(app.world().get::<KuiView>(cycle_mount).unwrap().status(), KuiViewStatus::Failed(message) if message.contains("recursive component import") && message.contains("part.kui -> part.kui"))
    );
    let button = format!(
        r#"{COMPONENT}<event name="activate"/><button on-click="emit.activate"/></component>"#
    );
    let main = format!(
        r#"{VIEW}<import name="part" src="part.kui"/><button on-click="outer"><use component="part"/></button></view>"#
    );
    let mut app = test_support::app(&[("main.kui", &main), ("part.kui", &button)]);
    let mount = mount(&mut app, BTreeMap::new());
    assert!(
        matches!(app.world().get::<KuiView>(mount).unwrap().status(), KuiViewStatus::Failed(message) if message.contains("nested buttons"))
    );
}

#[test]
fn parser_rejects_undeclared_props_and_owner_coupling() {
    for body in [
        r#"<text bind:text="owner.label"/>"#,
        r#"<text bind:text="props.unknown"/>"#,
        r#"<text bind:text="props.label.field"/>"#,
    ] {
        let error = parse(
            "component.kui",
            &format!(r#"{COMPONENT}<prop name="label" type="string"/>{body}</component>"#),
        )
        .unwrap_err();
        assert!(error.to_string().contains("component.kui"));
        assert!(error.message.contains("binding") || error.message.contains("prop"));
    }
}

#[test]
fn component_names_allow_hyphens_but_prop_names_remain_binding_identifiers() {
    let definition = parse("main.kui", &format!(r#"{VIEW}<import name="stat-bar" src="stat-bar.kui"/><use component="stat-bar"/></view>"#)).unwrap();
    assert_eq!(
        definition.imports.get("stat-bar").map(String::as_str),
        Some("stat-bar.kui")
    );
    assert!(
        parse(
            "part.kui",
            &format!(
                r#"{COMPONENT}<prop name="bad-name" type="number" default="1"/><node/></component>"#
            )
        )
        .unwrap_err()
        .message
        .contains("identifiers")
    );
}

#[derive(Resource, Default)]
struct Actions(Vec<KuiAction>);

#[test]
fn nested_events_route_current_payload_to_the_original_mount_and_owner() {
    let main = format!(
        r#"{VIEW}<import name="outer" src="outer.kui"/><use component="outer"><prop name="id" bind:value="selection"/><on name="chosen" action="inspect"/></use></view>"#
    );
    let outer = format!(
        r#"{COMPONENT}<import name="inner" src="inner.kui"/><prop name="id" type="scalar"/><event name="chosen"/><node><use component="inner"><prop name="id" bind:value="props.id"/><on name="activate" action="emit.chosen"/></use></node></component>"#
    );
    let inner = format!(
        r#"{COMPONENT}<prop name="id" type="scalar"/><event name="activate"/><button on-click="emit.activate" bind:value="props.id"><slot name="label"><text>Select</text></slot></button></component>"#
    );
    let mut app = test_support::app(&[
        ("main.kui", &main),
        ("outer.kui", &outer),
        ("inner.kui", &inner),
    ]);
    app.init_resource::<Actions>().add_observer(
        |action: On<KuiAction>, mut actions: ResMut<Actions>| actions.0.push(action.clone()),
    );
    let mount = mount(
        &mut app,
        BTreeMap::from([("selection".into(), KuiValue::Integer(7))]),
    );
    let owner = app.world().get::<KuiView>(mount).unwrap().owner;
    let button = app
        .world_mut()
        .query_filtered::<Entity, With<Button>>()
        .single(app.world())
        .unwrap();
    crate::tests::click(
        &mut app,
        button,
        bevy::picking::pointer::PointerButton::Primary,
    );
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert("selection".into(), KuiValue::Integer(9));
    app.update();
    crate::tests::click(
        &mut app,
        button,
        bevy::picking::pointer::PointerButton::Primary,
    );
    let actions = &app.world().resource::<Actions>().0;
    assert_eq!(
        actions
            .iter()
            .map(|a| a.value.as_deref())
            .collect::<Vec<_>>(),
        [Some("7"), Some("9")]
    );
    assert!(
        actions
            .iter()
            .all(|a| a.entity == owner && a.view == mount && a.action == "inspect")
    );
}

#[test]
fn component_dependency_changes_reload_consumers_and_recover_after_read_failure() {
    use bevy::asset::io::{
        AssetSourceBuilder, AssetSourceEvent, AssetSourceId, AssetWatcher,
        memory::{Dir, MemoryAssetReader},
    };
    let main =
        format!(r#"{VIEW}<import name="part" src="part.kui"/><use component="part"/></view>"#);
    let source = |label| format!("{COMPONENT}<text>{label}</text></component>");
    let directory = Dir::default();
    directory.insert_asset_text(std::path::Path::new("main.kui"), &main);
    directory.insert_asset_text(std::path::Path::new("part.kui"), &source("Before"));
    let reader = MemoryAssetReader {
        root: directory.clone(),
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    struct Watcher;
    impl AssetWatcher for Watcher {}
    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(reader.clone())).with_watcher(move |events| {
            sender.send(events).unwrap();
            Some(Box::new(Watcher))
        }),
    );
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        bevy::time::TimePlugin,
        AssetPlugin {
            watch_for_changes_override: Some(true),
            use_asset_processor_override: Some(false),
            ..default()
        },
    ));
    app.init_asset::<Image>().add_plugins(KuiPlugin);
    let events = receiver.recv().unwrap();
    let mount = mount(&mut app, BTreeMap::new());
    let initial = app.world().get::<KuiView>(mount).unwrap().roots()[0];
    assert_eq!(texts(&mut app), ["Before"]);
    directory.insert_asset_text(std::path::Path::new("part.kui"), &source("After"));
    events
        .send_blocking(AssetSourceEvent::ModifiedAsset("part.kui".into()))
        .unwrap();
    crate::tests::wait_for(&mut app, |app| texts(app) == ["After"]);
    assert!(app.world().get_entity(initial).is_err());
    directory.remove_asset(std::path::Path::new("part.kui"));
    events
        .send_blocking(AssetSourceEvent::ModifiedAsset("part.kui".into()))
        .unwrap();
    crate::tests::wait_for(
        &mut app,
        |app| matches!(app.world().get::<KuiView>(mount).unwrap().status(), KuiViewStatus::Failed(message) if message.contains("part.kui")),
    );
    assert!(texts(&mut app).is_empty());
    directory.insert_asset_text(std::path::Path::new("part.kui"), &source("Restored"));
    events
        .send_blocking(AssetSourceEvent::AddedAsset("part.kui".into()))
        .unwrap();
    crate::tests::wait_for(&mut app, |app| texts(app) == ["Restored"]);
    assert_eq!(
        app.world().get::<KuiView>(mount).unwrap().status(),
        &KuiViewStatus::Ready
    );
}

#[test]
fn buttons_authored_in_nested_slots_emit_the_declaring_components_event() {
    let main = format!(
        r#"{VIEW}<import name="outer" src="outer.kui"/><use component="outer"><on name="chosen" action="inspect"/></use></view>"#
    );
    let outer = format!(
        r#"{COMPONENT}<import name="inner" src="inner.kui"/><prop name="id" type="integer" default="9"/><event name="chosen"/><use component="inner"><fill slot="body"><button on-click="emit.chosen" bind:value="props.id"><text>Choose</text></button></fill></use></component>"#
    );
    let inner = format!(r#"{COMPONENT}<node><slot name="body"/></node></component>"#);
    let mut app = test_support::app(&[
        ("main.kui", &main),
        ("outer.kui", &outer),
        ("inner.kui", &inner),
    ]);
    app.init_resource::<Actions>().add_observer(
        |action: On<KuiAction>, mut actions: ResMut<Actions>| actions.0.push(action.clone()),
    );
    let mount = mount(&mut app, BTreeMap::new());
    let button = app
        .world_mut()
        .query_filtered::<Entity, With<Button>>()
        .single(app.world())
        .unwrap();
    crate::tests::click(
        &mut app,
        button,
        bevy::picking::pointer::PointerButton::Primary,
    );
    let actions = &app.world().resource::<Actions>().0;
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].action, "inspect");
    assert_eq!(actions[0].value.as_deref(), Some("9"));
    assert_eq!(actions[0].view, mount);
}

#[test]
fn directly_mounted_components_enforce_types_and_emit_public_event_names() {
    let source = format!(
        r#"{COMPONENT}<prop name="label" type="string"/><prop name="id" type="integer"/><event name="activate"/><button on-click="emit.activate" bind:value="props.id"><text bind:text="props.label">Fallback</text></button></component>"#
    );
    let mut app = test_support::app(&[("main.kui", &source)]);
    app.init_resource::<Actions>().add_observer(
        |action: On<KuiAction>, mut actions: ResMut<Actions>| actions.0.push(action.clone()),
    );
    let mount = mount(
        &mut app,
        BTreeMap::from([(
            "props".into(),
            KuiValue::Record(BTreeMap::from([
                ("label".into(), KuiValue::Integer(123)),
                ("id".into(), KuiValue::Integer(7)),
            ])),
        )]),
    );
    assert_eq!(texts(&mut app), ["Fallback"]);
    let button = app.world().get::<KuiView>(mount).unwrap().roots()[0];
    crate::tests::click(
        &mut app,
        button,
        bevy::picking::pointer::PointerButton::Primary,
    );
    assert_eq!(app.world().resource::<Actions>().0[0].action, "activate");
    app.world_mut()
        .get_mut::<KuiView>(mount)
        .unwrap()
        .data
        .insert(
            "props".into(),
            KuiValue::Record(BTreeMap::from([
                ("label".into(), KuiValue::String("Recovered".into())),
                ("id".into(), KuiValue::Integer(9)),
            ])),
        );
    app.update();
    assert_eq!(texts(&mut app), ["Recovered"]);
    assert_eq!(
        app.world().get::<KuiView>(mount).unwrap().roots(),
        &[button]
    );
}

#[test]
fn instance_ids_and_classes_reach_visual_roots_through_nested_components() {
    let main = format!(
        r#"{VIEW}<import name="outer" src="outer.kui"/><use component="outer" id="caller" class="theme"/></view>"#
    );
    let outer = format!(
        r#"{COMPONENT}<import name="inner" src="inner.kui"/><use component="inner" id="local" class="middle"/></component>"#
    );
    let inner = format!(r#"{COMPONENT}<node id="inside" class="base"/></component>"#);
    let mut app = test_support::app(&[
        ("main.kui", &main),
        ("outer.kui", &outer),
        ("inner.kui", &inner),
    ]);
    let mount = mount(&mut app, BTreeMap::new());
    let root = app.world().get::<KuiView>(mount).unwrap().roots()[0];
    assert_eq!(app.world().get::<Name>(root).unwrap().as_str(), "caller");
    let classes = app
        .world()
        .get::<bevy_flair::prelude::ClassList>(root)
        .unwrap();
    assert!(classes.contains("base") && classes.contains("middle") && classes.contains("theme"));
    assert_eq!(
        app.world_mut().query::<&Node>().iter(app.world()).count(),
        1
    );
}
