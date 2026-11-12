use std::{num::NonZeroUsize, time::Duration};

use bevy_app::App;
use bevy_ecs::event::Event;

use super::{CompileError, compile, parse};
use crate::{
    BuildError, DatamodelOperation, InstanceStatus, LuauValue, RuntimeFault, StateTreeDefinition,
    StateTreeExt, definition::TimerDuration, test_harness::TestApp,
};

#[derive(Event, crate::StateTreeEvent)]
#[state_tree(name = "change", broadcast, data = change_data)]
struct Change {
    seconds: f64,
}

fn change_data(event: &Change) -> LuauValue {
    LuauValue::Record([("seconds".into(), LuauValue::Number(event.seconds))].into())
}

fn configure(app: &mut App) {
    app.add_state_tree_event::<Change>();
}

fn execution(body: &str) -> TestApp {
    TestApp::new(configure, definition(body), NonZeroUsize::new(32).unwrap())
}

fn definition(body: &str) -> StateTreeDefinition {
    compile(&parse("after.scxml", &source(body)).unwrap()).unwrap()
}

fn source(body: &str) -> String {
    format!(
        r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:ist="urn:inslag-state-tree" datamodel="luau">{body}</scxml>"#
    )
}

#[test]
fn constant_seconds_compile_to_fixed_durations_without_a_datamodel() {
    for (expression, expected) in [
        ("4.8", Duration::from_millis(4800)),
        ("1 / 4", Duration::from_millis(250)),
        ("1e-3", Duration::from_millis(1)),
        ("0x2", Duration::from_secs(2)),
    ] {
        let source = source(&format!(
            r#"<state id="waiting"><transition ist:after="{expression}" target="done"/></state><final id="done"/>"#
        ))
        .replace(r#" datamodel="luau""#, "");
        let definition = compile(&parse("constant.scxml", &source).unwrap()).unwrap();
        assert!(definition.compiled.datamodel.is_none());
        assert_eq!(
            definition.compiled.timers[0].duration,
            TimerDuration::Fixed(expected),
            "{expression}"
        );
    }
}

#[test]
fn rejects_old_units_and_malformed_expressions_at_the_transition() {
    for expression in ["4.8s", "4800ms", "", "remaining +"] {
        let source = source(&format!(
            r#"<state><transition ist:after="{expression}" target="done"/></state><final id="done"/>"#
        ));
        assert!(
            matches!(
                compile(&parse("invalid.scxml", &source).unwrap()),
                Err(CompileError::InvalidLuau { span_start, .. })
                    if span_start == source.find("<transition").unwrap()
            ),
            "{expression}"
        );
    }
}

#[test]
fn rejects_invalid_constant_durations() {
    for expression in ["0", "-1", "1e30", "1e-20", "true", "nil", "'1'"] {
        let source = source(&format!(
            r#"<state><transition ist:after="{expression}" target="done"/></state><final id="done"/>"#
        ));
        assert!(
            matches!(
                compile(&parse("invalid.scxml", &source).unwrap()),
                Err(CompileError::InvalidDocument { span_start, .. })
                    if span_start == source.find("<transition").unwrap()
            ),
            "{expression}"
        );
    }
}

#[test]
fn dynamic_after_requires_a_luau_datamodel() {
    let source = source(
        r#"<state><transition ist:after="remaining" target="done"/></state><final id="done"/>"#,
    )
    .replace(r#" datamodel="luau""#, "");
    assert!(matches!(
        compile(&parse("missing-datamodel.scxml", &source).unwrap()),
        Err(CompileError::Build(BuildError::MissingDataModel))
    ));
}

#[test]
fn samples_after_entry_once_and_does_not_wake_before_the_deadline() {
    let mut execution = execution(
        r#"
        <datamodel><data id="delay" expr="99"/></datamodel>
        <state id="waiting">
            <onentry><assign location="delay" expr="2"/></onentry>
            <transition ist:after="delay" target="done"/>
            <transition event="change"><assign location="delay" expr="_event.data.seconds"/></transition>
        </state>
        <final id="done"/>
        "#,
    );
    assert_eq!(execution.data("delay"), Some(&LuauValue::Integer(2)));
    assert!(!execution.advance_time(Duration::from_millis(500)));
    execution.process_event(Change { seconds: 10. });
    let trace = execution.trace().as_str().to_owned();
    for _ in 0..149 {
        assert!(!execution.advance_time(Duration::from_millis(10)));
    }
    assert_eq!(execution.trace().as_str(), trace);
    assert_eq!(execution.active_state_names().last(), Some("waiting"));
    assert!(execution.advance_time(Duration::from_millis(10)));
    assert_eq!(execution.status(), &InstanceStatus::Complete);
}

#[test]
fn cancellation_and_reentry_resample_the_expression() {
    let mut execution = execution(
        r#"
        <datamodel><data id="delay" expr="1"/></datamodel>
        <state id="waiting">
            <transition ist:after="delay" target="done"/>
            <transition event="change" target="outside"/>
        </state>
        <state id="outside">
            <transition event="change" target="waiting"><assign location="delay" expr="_event.data.seconds"/></transition>
        </state>
        <final id="done"/>
        "#,
    );
    assert!(!execution.advance_time(Duration::from_millis(500)));
    assert!(execution.process_event(Change { seconds: 0. }));
    assert!(!execution.advance_time(Duration::from_secs(2)));
    assert!(execution.process_event(Change { seconds: 3. }));
    assert!(!execution.advance_time(Duration::from_millis(2999)));
    assert_eq!(execution.active_state_names().last(), Some("waiting"));
    assert!(execution.advance_time(Duration::from_millis(1)));
    assert_eq!(execution.status(), &InstanceStatus::Complete);
}

#[test]
fn delay_expressions_can_read_configuration_and_the_triggering_event() {
    let mut execution = execution(
        r#"
        <state id="idle"><transition event="change" target="waiting"/></state>
        <state id="waiting"><transition ist:after="config.factor * _event.data.seconds" target="done"/></state>
        <final id="done"/>
        "#,
    );
    let entity = execution.entity();
    execution
        .app_mut()
        .world_mut()
        .entity_mut(entity)
        .insert(crate::StateTreeConfiguration::new([(
            "factor".into(),
            LuauValue::Number(0.5),
        )]));
    execution
        .app_mut()
        .world_mut()
        .run_schedule(bevy_app::Update);
    execution
        .app_mut()
        .world_mut()
        .run_schedule(bevy_app::Update);
    assert!(execution.process_event(Change { seconds: 4. }));
    assert!(!execution.advance_time(Duration::from_millis(1999)));
    assert!(execution.advance_time(Duration::from_millis(1)));
    assert_eq!(execution.status(), &InstanceStatus::Complete);
}

#[test]
fn parallel_dynamic_timers_have_independent_deadlines() {
    let mut execution = execution(
        r#"
        <datamodel><data id="left_delay" expr="1"/><data id="right_delay" expr="2"/></datamodel>
        <parallel id="both">
            <state id="left">
                <state id="left_waiting"><transition ist:after="left_delay" target="left_done"/></state>
                <final id="left_done"/>
            </state>
            <state id="right">
                <state id="right_waiting"><transition ist:after="right_delay" target="right_done"/></state>
                <final id="right_done"/>
            </state>
        </parallel>
        "#,
    );
    assert!(execution.advance_time(Duration::from_secs(1)));
    assert!(
        execution
            .active_state_names()
            .any(|name| name == "left_done")
    );
    assert!(
        execution
            .active_state_names()
            .any(|name| name == "right_waiting")
    );
    assert!(execution.advance_time(Duration::from_secs(1)));
    assert!(
        execution
            .active_state_names()
            .any(|name| name == "right_done")
    );
}

#[test]
fn targetless_dynamic_timer_fires_once_after_a_long_frame() {
    let mut execution = execution(
        r#"
        <datamodel><data id="delay" expr="2"/><data id="fired" expr="0"/></datamodel>
        <state>
            <transition ist:after="delay"><assign location="fired" expr="fired + 1"/></transition>
        </state>
        "#,
    );
    assert!(execution.advance_time(Duration::from_secs(60)));
    assert_eq!(execution.data("fired"), Some(&LuauValue::Integer(1)));
    assert!(!execution.advance_time(Duration::from_secs(60)));
    assert_eq!(execution.data("fired"), Some(&LuauValue::Integer(1)));
}

#[test]
fn shared_definitions_use_instance_local_delay_values() {
    let mut execution = execution(
        r#"
        <datamodel><data id="delay" expr="1"/></datamodel>
        <state><transition ist:after="delay" target="done"/></state>
        <final id="done"/>
        "#,
    );
    let handle = execution.instance().definition().clone();
    let second = execution
        .app_mut()
        .world_mut()
        .spawn(crate::StateTreeInstance::new_with_initial_data(
            handle,
            crate::StateTreeDataOverrides::try_new([("delay", LuauValue::Number(2.))]).unwrap(),
        ))
        .id();
    execution
        .app_mut()
        .world_mut()
        .run_schedule(bevy_app::Update);
    assert!(execution.advance_time(Duration::from_secs(1)));
    assert_eq!(execution.status(), &InstanceStatus::Complete);
    assert_eq!(
        execution
            .app()
            .world()
            .get::<crate::StateTreeInstance>(second)
            .unwrap()
            .status(),
        &InstanceStatus::Running
    );
    execution.advance_time(Duration::from_secs(1));
    assert_eq!(
        execution
            .app()
            .world()
            .get::<crate::StateTreeInstance>(second)
            .unwrap()
            .status(),
        &InstanceStatus::Complete
    );
}

#[test]
fn invalid_dynamic_results_fault_only_the_instance_and_cancel_armed_timers() {
    for value in [
        "0",
        "-1",
        "math.huge",
        "0/0",
        "1e30",
        "1e-20",
        "true",
        "nil",
        "'1'",
        "{}",
    ] {
        let body = format!(
            r#"
            <datamodel><data id="delay" expr="{value}"/></datamodel>
            <state id="waiting">
                <transition ist:after="1" target="done"/>
                <transition ist:after="delay" target="done"/>
            </state>
            <final id="done"/>
            "#
        );
        let mut execution = execution(&body);
        assert!(
            matches!(
                execution.status(),
                InstanceStatus::Faulted(RuntimeFault::Datamodel {
                    operation: DatamodelOperation::TimerDuration,
                    span_start,
                    ..
                }) if *span_start == source(&body).find(r#"<transition ist:after="delay""#).unwrap()
            ),
            "{value}: {:?}",
            execution.status()
        );
        assert!(!execution.advance_time(Duration::from_secs(10)));
    }
}

#[test]
fn delay_evaluation_errors_report_timer_context() {
    let execution = execution(
        r#"<state><transition ist:after="config.missing.seconds" target="done"/></state><final id="done"/>"#,
    );
    assert!(matches!(
        execution.status(),
        InstanceStatus::Faulted(RuntimeFault::Datamodel {
            operation: DatamodelOperation::TimerDuration,
            ..
        })
    ));
}
