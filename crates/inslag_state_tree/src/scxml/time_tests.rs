use std::{num::NonZeroUsize, time::Duration};

use bevy_app::{App, Update};
use bevy_ecs::event::Event;
use bevy_time::{Time, TimeUpdateStrategy, Virtual};

use super::{compile, parse};
use crate::{InstanceStatus, LuauValue, StateTreeExt, StateTreeInstance, test_harness::TestApp};

#[derive(Event, crate::StateTreeEvent)]
#[state_tree(name = "sample", broadcast, data = sample_data)]
struct Sample;

fn sample_data(_: &Sample) -> LuauValue {
    LuauValue::Number(2.)
}

fn configure(app: &mut App) {
    app.add_state_tree_event::<Sample>();
}

fn execution(body: &str) -> TestApp {
    let source = format!(
        r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:ist="urn:inslag-state-tree" datamodel="luau">{body}</scxml>"#
    );
    let definition = compile(&parse("time.scxml", &source).unwrap()).unwrap();
    TestApp::new(configure, definition, NonZeroUsize::new(32).unwrap())
}

fn number(value: Option<&LuauValue>) -> f64 {
    match value {
        Some(LuauValue::Number(value)) => *value,
        Some(LuauValue::Integer(value)) => *value as f64,
        other => panic!("expected time, got {other:?}"),
    }
}

#[test]
fn time_is_sampled_only_when_the_chart_runs_in_native_and_vm_expressions() {
    let mut execution = execution(
        r#"
        <datamodel>
            <data id="initialized" expr="_time"/>
            <data id="entered" expr="-1"/>
            <data id="native" expr="-1"/>
            <data id="vm" expr="-1"/>
            <data id="combined" expr="-1"/>
        </datamodel>
        <state>
            <onentry><assign location="entered" expr="_time"/></onentry>
            <transition event="sample" cond="_time >= 1.25">
                <assign location="native" expr="_time"/>
                <assign location="vm" expr="(function() return _time end)()"/>
                <assign location="combined" expr="_time + _event.data + (config.offset or 0)"/>
            </transition>
        </state>
        "#,
    );
    assert_eq!(number(execution.data("initialized")), 0.);
    assert_eq!(number(execution.data("entered")), 0.);
    let trace = execution.trace().as_str().to_owned();
    for _ in 0..125 {
        assert!(!execution.advance_time(Duration::from_millis(10)));
    }
    assert_eq!(execution.trace().as_str(), trace);
    assert_eq!(number(execution.data("native")), -1.);
    assert!(execution.process_event(Sample));
    assert_eq!(number(execution.data("native")), 1.25);
    assert_eq!(number(execution.data("vm")), 1.25);
    assert_eq!(number(execution.data("combined")), 3.25);
}

#[test]
fn initialization_uses_current_simulation_time_not_instance_age() {
    let mut execution =
        execution(r#"<datamodel><data id="started" expr="_time"/></datamodel><state/>"#);
    execution.advance_time(Duration::from_millis(5250));
    let handle = execution.instance().definition().clone();
    let second = execution
        .app_mut()
        .world_mut()
        .spawn(StateTreeInstance::new(handle))
        .id();
    execution.app_mut().world_mut().run_schedule(Update);
    assert_eq!(number(execution.data("started")), 0.);
    assert_eq!(
        number(
            execution
                .app()
                .world()
                .get::<StateTreeInstance>(second)
                .unwrap()
                .data("started")
        ),
        5.25
    );
}

#[test]
fn deadlines_use_time_and_late_delivery_reads_actual_processing_time() {
    let mut execution = execution(
        r#"
        <datamodel><data id="deadline" expr="0"/><data id="finished" expr="0"/></datamodel>
        <state id="idle"><transition event="sample" target="waiting"/></state>
        <state id="waiting">
            <onentry><assign location="deadline" expr="_time + 2"/></onentry>
            <transition ist:after="deadline - _time" target="done"/>
        </state>
        <final id="done"><onentry><assign location="finished" expr="_time"/></onentry></final>
        "#,
    );
    execution.advance_time(Duration::from_secs(5));
    execution.process_event(Sample);
    assert_eq!(number(execution.data("deadline")), 7.);
    assert!(!execution.advance_time(Duration::from_secs(1)));
    assert!(execution.advance_time(Duration::from_secs(4)));
    assert_eq!(execution.status(), &InstanceStatus::Complete);
    assert_eq!(number(execution.data("finished")), 10.);
}

#[test]
fn virtual_time_pause_and_speed_apply_without_a_separate_clock() {
    let mut execution = execution(
        r#"<datamodel><data id="sampled" expr="0"/></datamodel>
        <state><transition event="sample"><assign location="sampled" expr="_time"/></transition></state>"#,
    );
    execution
        .app_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
    execution
        .app_mut()
        .world_mut()
        .resource_mut::<Time<Virtual>>()
        .pause();
    execution.app_mut().update();
    execution.process_event(Sample);
    assert_eq!(number(execution.data("sampled")), 0.);
    {
        let mut time = execution
            .app_mut()
            .world_mut()
            .resource_mut::<Time<Virtual>>();
        time.unpause();
        time.set_relative_speed(2.);
    }
    execution.app_mut().update();
    execution.process_event(Sample);
    assert_eq!(number(execution.data("sampled")), 0.2);
}

#[test]
fn time_cannot_be_declared_or_assigned_as_datamodel_state() {
    for body in [
        r#"<datamodel><data id="_time" expr="0"/></datamodel><state/>"#,
        r#"<state><onentry><assign location="_time" expr="0"/></onentry></state>"#,
    ] {
        let source = format!(
            r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" datamodel="luau">{body}</scxml>"#
        );
        assert!(compile(&parse("reserved-time.scxml", &source).unwrap()).is_err());
    }
}
