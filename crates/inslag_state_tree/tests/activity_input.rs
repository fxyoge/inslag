use std::{collections::BTreeMap, marker::PhantomData};

use bevy_app::{App, Update};
use bevy_asset::{AssetPlugin, Assets};
use bevy_ecs::prelude::*;
use bevy_math::Vec2;
use bevy_time::TimePlugin;
use inslag_state_tree::prelude::*;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        TimePlugin,
        AssetPlugin::default(),
        StateTreePlugin::new(Update),
    ));
    app
}

fn spawn(app: &mut App, name: &str, arguments: &str) -> Entity {
    let source = format!(
        r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:ist="urn:inslag-state-tree" datamodel="luau"><state id="active"><ist:activity id="task" name="{name}">{arguments}</ist:activity></state></scxml>"#
    );
    let definition = inslag_state_tree::scxml::compile(
        &inslag_state_tree::scxml::parse("input.scxml", &source).unwrap(),
    )
    .unwrap();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition);
    app.world_mut().spawn(StateTreeInstance::new(handle)).id()
}

fn started<A: StateTreeActivity>(app: &mut App) -> Vec<(StateTreeActivityToken<A>, A)> {
    app.world_mut()
        .resource_mut::<Messages<StateTreeActivityLifecycle<A>>>()
        .drain()
        .filter_map(|message| match message {
            StateTreeActivityLifecycle::Started { token, activity } => Some((token, activity)),
            StateTreeActivityLifecycle::Cancelled { .. } => None,
        })
        .collect()
}

#[derive(Debug, Default, PartialEq, StateTreeInput)]
#[state_tree(rename_all = "snake_case")]
enum Mode {
    #[default]
    WorldSpace,
    #[state_tree(rename = "layout")]
    LayoutSpace,
}

#[derive(Debug, PartialEq, StateTreeInput)]
struct Settings {
    points: Vec<Vec2>,
    weights: BTreeMap<String, u16>,
}

#[derive(Debug, StateTreeInput)]
struct Request {
    #[state_tree(rename = "count")]
    amount: u32,
    #[state_tree(default)]
    enabled: bool,
    #[state_tree(default = "fallback".into())]
    label: String,
    #[state_tree(default)]
    mode: Mode,
    value: Option<String>,
    settings: Settings,
    bounds: [f32; 4],
}

impl StateTreeActivity for Request {
    type Output = ();
    type Error = ();
}

const REQUIRED: &str = r#"
    <ist:arg name="count" expr="3"/>
    <ist:arg name="settings" expr="{points={{1,2},{x=3,y=4}},weights={a=7}}"/>
    <ist:arg name="bounds" expr="{0,1,2,3}"/>
"#;

#[test]
fn decodes_required_defaulted_renamed_nested_and_enum_values() {
    let mut app = app();
    app.add_state_tree_activity::<Request>("request");
    let owner = spawn(&mut app, "request", REQUIRED);
    app.update();
    let (token, request) = started::<Request>(&mut app).pop().unwrap();
    assert_eq!(token.entity(), owner);
    assert_eq!(request.amount, 3);
    assert!(!request.enabled);
    assert_eq!(request.label, "fallback");
    assert_eq!(request.mode, Mode::WorldSpace);
    assert_eq!(request.value, None);
    assert_eq!(request.bounds, [0., 1., 2., 3.]);
    assert_eq!(
        request.settings.points,
        [Vec2::new(1., 2.), Vec2::new(3., 4.)]
    );
    assert_eq!(request.settings.weights, BTreeMap::from([("a".into(), 7)]));

    spawn(
        &mut app,
        "request",
        &format!(
            r#"{REQUIRED}<ist:arg name="enabled" expr="true"/><ist:arg name="label" expr="'custom'"/><ist:arg name="mode" expr="'layout'"/><ist:arg name="value" expr="'present'"/>"#
        ),
    );
    app.update();
    let (_, request) = started::<Request>(&mut app).pop().unwrap();
    assert!(request.enabled);
    assert_eq!(request.label, "custom");
    assert_eq!(request.mode, Mode::LayoutSpace);
    assert_eq!(request.value.as_deref(), Some("present"));
}

#[test]
fn omitted_and_nil_optional_fields_are_equivalent() {
    let mut app = app();
    app.add_state_tree_activity::<Request>("request");
    spawn(&mut app, "request", REQUIRED);
    spawn(
        &mut app,
        "request",
        &format!(r#"{REQUIRED}<ist:arg name="value" expr="nil"/>"#),
    );
    app.update();
    let requests = started::<Request>(&mut app);
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|(_, request)| request.value.is_none()));
}

#[test]
fn invalid_arguments_fault_before_publishing_a_start() {
    for (arguments, expected) in [
        (REQUIRED.replace("name=\"count\"", "name=\"typo\""), "count"),
        (
            format!(r#"{REQUIRED}<ist:arg name="unknown" expr="1"/>"#),
            "unknown",
        ),
        (REQUIRED.replace("expr=\"3\"", "expr=\"'three'\""), "count"),
        (REQUIRED.replace("expr=\"3\"", "expr=\"-1\""), "count"),
        (REQUIRED.replace("{0,1,2,3}", "{0,1,2}"), "bounds"),
        (
            REQUIRED.replace("weights={a=7}", "weights={a=70000}"),
            "settings",
        ),
        (
            REQUIRED.replace("weights={a=7}", "weights={a=7},unknown=1"),
            "settings",
        ),
        (
            format!(r#"{REQUIRED}<ist:arg name="mode" expr="'typo'"/>"#),
            "mode",
        ),
        (
            format!(r#"{REQUIRED}<ist:arg name="enabled" expr="nil"/>"#),
            "enabled",
        ),
    ] {
        let mut app = app();
        app.add_state_tree_activity::<Request>("request");
        let owner = spawn(&mut app, "request", &arguments);
        app.update();
        let status = app
            .world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .status();
        assert!(
            matches!(status, InstanceStatus::Faulted(RuntimeFault::Activity { argument: Some(argument), .. }) if argument.as_ref() == expected),
            "{arguments}: {status:?}"
        );
        assert!(started::<Request>(&mut app).is_empty());
    }
}

#[derive(StateTreeInput)]
struct ContextRequest<D> {
    #[state_tree(default_owner)]
    target: Entity,
    #[state_tree(activity_id)]
    id: String,
    #[state_tree(skip)]
    marker: PhantomData<fn() -> D>,
}

impl<D: 'static> StateTreeActivity for ContextRequest<D> {
    type Output = ();
    type Error = ();
}

#[test]
fn context_injection_and_generic_markers_need_no_consumer_parser() {
    struct Domain;
    let mut app = app();
    app.add_state_tree_activity::<ContextRequest<Domain>>("context");
    let other = app.world_mut().spawn_empty().id();
    let owner = spawn(&mut app, "context", "");
    let overridden = spawn(
        &mut app,
        "context",
        &format!(r#"<ist:arg name="target" expr="'{}'"/>"#, other.to_bits()),
    );
    app.update();
    let requests = started::<ContextRequest<Domain>>(&mut app);
    assert_eq!(requests.len(), 2);
    for (token, request) in requests {
        assert_eq!(request.id, "task");
        assert_eq!(
            request.target,
            if token.entity() == owner {
                owner
            } else {
                assert_eq!(token.entity(), overridden);
                other
            }
        );
    }
}

#[derive(StateTreeInput)]
struct Converted(#[state_tree(rename = "amount")] u32);

impl StateTreeActivity for Converted {
    type Output = ();
    type Error = ();
}

#[test]
fn decoded_activities_start_without_domain_admission_checks() {
    let mut app = app();
    app.add_state_tree_activity::<Converted>("canonical");
    app.world_mut()
        .add_state_tree_activity::<Converted>("alternate");
    let canonical = spawn(
        &mut app,
        "canonical",
        r#"<ist:arg name="amount" expr="5"/>"#,
    );
    let alternate = spawn(
        &mut app,
        "alternate",
        r#"<ist:arg name="amount" expr="6"/>"#,
    );
    let invalid = spawn(
        &mut app,
        "canonical",
        r#"<ist:arg name="amount" expr="0"/>"#,
    );
    app.update();
    let requests = started::<Converted>(&mut app);
    assert_eq!(requests.len(), 3);
    for (token, request) in requests {
        assert_eq!(
            request.0,
            if token.entity() == canonical {
                5
            } else if token.entity() == alternate {
                assert_eq!(token.entity(), alternate);
                6
            } else {
                assert_eq!(token.entity(), invalid);
                0
            }
        );
    }
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(invalid)
            .unwrap()
            .status(),
        &InstanceStatus::Running
    );
}

#[derive(StateTreeInput)]
struct LazyDefault {
    #[state_tree(default = panic!("default must not run when supplied"))]
    value: u32,
}

#[test]
fn defaults_are_lazy() {
    let value = LazyDefault::from_state_tree_value(LuauValue::Record(BTreeMap::from([(
        "value".into(),
        LuauValue::Integer(7),
    )])))
    .unwrap();
    assert_eq!(value.value, 7);
}

#[test]
fn record_projections_can_ignore_unrelated_fields_without_weakening_field_types() {
    #[derive(StateTreeInput)]
    #[state_tree(allow_unknown_fields)]
    struct Projection {
        value: u32,
    }
    let record = |value| {
        LuauValue::Record(BTreeMap::from([
            ("value".into(), value),
            ("unrelated".into(), LuauValue::Boolean(true)),
        ]))
    };
    assert_eq!(
        Projection::from_state_tree_value(record(LuauValue::Integer(7)))
            .unwrap()
            .value,
        7
    );
    assert!(Projection::from_state_tree_value(record(LuauValue::String("bad".into()))).is_err());
}

#[test]
fn numeric_and_entity_conversions_are_strict() {
    assert_eq!(
        f32::from_state_tree_value(LuauValue::Number(f64::INFINITY)).unwrap(),
        f32::INFINITY
    );
    assert!(
        f32::from_state_tree_value(LuauValue::Number(f64::NAN))
            .unwrap()
            .is_nan()
    );
    assert!(u16::from_state_tree_value(LuauValue::Number(1.)).is_err());
    assert!(u16::from_state_tree_value(LuauValue::Integer(-1)).is_err());
    assert!(Entity::from_state_tree_value(LuauValue::String("not-an-entity".into())).is_err());
    assert!(Entity::from_state_tree_value(LuauValue::Integer(1)).is_err());
    assert!(
        Vec2::from_state_tree_value(LuauValue::Record(BTreeMap::from([
            ("x".into(), LuauValue::Integer(1)),
            ("y".into(), LuauValue::Integer(2)),
            ("extra".into(), LuauValue::Integer(3)),
        ])))
        .is_err()
    );
}
