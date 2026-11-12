use super::*;
use crate::{
    runtime::test_support,
    tests::{HEADER, click, wait_for},
};
use bevy::{ecs::system::RunSystemOnce, input_focus::InputFocus};
use inslag_state_tree::{StateTreeDefinition, StateTreePlugin, scxml};

#[derive(EntityEvent, inslag_state_tree::StateTreeEvent)]
#[state_tree(name = "test.input", data = input_data)]
struct Input {
    entity: Entity,
    command: String,
}

fn input_data(event: &Input) -> LuauValue {
    LuauValue::String(event.command.clone())
}

const CHART: &str = r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:bst="urn:inslag-state-tree" datamodel="luau" initial="active">
  <datamodel>
    <data id="disabled" expr="false"/><data id="shown" expr="true"/><data id="value" expr="'old'"/>
    <data id="received" expr="'none'"/><data id="actions" expr="0"/>
    <data id="ready_count" expr="0"/><data id="focus_count" expr="0"/>
    <data id="rows" expr="{{id='oak'},{id='ash'}}"/>
  </datamodel>
  <state id="active">
    <bst:activity id="menu" name="ui.render"><bst:arg name="src" expr="'navigation.kui'"/></bst:activity>
    <bst:activity id="other" name="ui.render"><bst:arg name="src" expr="'other.kui'"/></bst:activity>
    <transition event="ui.render.ready"><assign location="ready_count" expr="ready_count + 1"/></transition>
    <transition event="ui.focus.changed"><assign location="focus_count" expr="focus_count + 1"/></transition>
    <transition event="test.input" cond="_event.data == 'next'"><bst:effect name="ui.focus"><bst:arg name="view" expr="'menu'"/><bst:arg name="direction" expr="'next'"/></bst:effect></transition>
    <transition event="test.input" cond="_event.data == 'previous'"><bst:effect name="ui.focus"><bst:arg name="view" expr="'menu'"/><bst:arg name="direction" expr="'previous'"/></bst:effect></transition>
    <transition event="test.input" cond="_event.data == 'activate'"><bst:effect name="ui.activate"><bst:arg name="view" expr="'menu'"/></bst:effect></transition>
    <transition event="test.input" cond="_event.data == 'other_next'"><bst:effect name="ui.focus"><bst:arg name="view" expr="'other'"/><bst:arg name="direction" expr="'next'"/></bst:effect></transition>
    <transition event="test.input" cond="_event.data == 'disable'"><assign location="disabled" expr="not disabled"/></transition>
    <transition event="test.input" cond="_event.data == 'hide'"><assign location="shown" expr="not shown"/></transition>
    <transition event="test.input" cond="_event.data == 'change'"><assign location="value" expr="'new'"/></transition>
    <transition event="test.input" cond="_event.data == 'invalid'"><assign location="value" expr="{}"/></transition>
    <transition event="test.input" cond="_event.data == 'reorder'"><assign location="rows" expr="{rows[2],rows[1]}"/></transition>
    <transition event="test.input" cond="_event.data == 'remove'"><assign location="rows" expr="{}"/></transition>
    <transition event="test.input" cond="_event.data == 'restart'" target="active"/>
    <transition event="test.input" cond="_event.data == 'stale'" target="active"><bst:effect name="ui.activate"><bst:arg name="view" expr="'menu'"/></bst:effect></transition>
    <transition event="ui.action"><assign location="received" expr="_event.data.view .. ':' .. _event.data.action .. ':' .. (_event.data.value or 'nil')"/><assign location="actions" expr="actions + 1"/></transition>
  </state>
</scxml>"#;

fn fixture(body: &str) -> (App, Entity) {
    let source = format!("{HEADER}{body}</view>");
    let other = format!(r#"{HEADER}<button id="other-button" on-click="other"/></view>"#);
    let mut app = test_support::app(&[("navigation.kui", &source), ("other.kui", &other)]);
    app.add_plugins((StateTreePlugin::new(Update), KuiStateTreePlugin))
        .add_state_tree_event::<Input>();
    let owner = spawn(&mut app);
    wait_for(&mut app, |app| number(app, owner, "ready_count") == 2);
    (app, owner)
}

fn spawn(app: &mut App) -> Entity {
    let definition = scxml::compile(&scxml::parse("navigation.scxml", CHART).unwrap()).unwrap();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<StateTreeDefinition>>()
        .add(definition);
    app.world_mut().spawn(StateTreeInstance::new(handle)).id()
}

fn input(app: &mut App, entity: Entity, command: &str) {
    queue(app, entity, command);
    app.update();
}

fn queue(app: &mut App, entity: Entity, command: &str) {
    let command = command.to_owned();
    app.world_mut()
        .run_system_once(move |mut events: EntityStateTreeEventWriter| {
            events.write(Input {
                entity,
                command: command.clone(),
            });
        })
        .unwrap();
}
fn number(app: &App, owner: Entity, key: &str) -> i64 {
    match app
        .world()
        .get::<StateTreeInstance>(owner)
        .unwrap()
        .data(key)
    {
        Some(LuauValue::Integer(value)) => *value,
        value => panic!("expected integer {key}: {value:?}"),
    }
}
fn received(app: &App, owner: Entity) -> &str {
    match app
        .world()
        .get::<StateTreeInstance>(owner)
        .unwrap()
        .data("received")
    {
        Some(LuauValue::String(value)) => value,
        value => panic!("expected received action: {value:?}"),
    }
}
fn focused_name(app: &App) -> &str {
    app.world()
        .get::<Name>(focused(app).unwrap())
        .unwrap()
        .as_str()
}

fn focused(app: &App) -> Option<Entity> {
    app.world().resource::<InputFocus>().get()
}

#[test]
fn navigation_uses_authored_order_not_ids_and_wraps_both_directions() {
    let (mut app, owner) = fixture(
        r#"<node><button id="z" on-click="z"/><button id="a" on-click="a"/><button id="priority" tab-index="1" on-click="first"/><button id="excluded" tab-index="-1" on-click="excluded"/></node>"#,
    );
    input(&mut app, owner, "activate");
    app.update();
    assert_eq!(
        number(&app, owner, "actions"),
        0,
        "no implicit activation target"
    );
    for expected in ["priority", "z", "a", "priority"] {
        input(&mut app, owner, "next");
        assert_eq!(focused_name(&app), expected);
    }
    input(&mut app, owner, "previous");
    assert_eq!(focused_name(&app), "a");
    input(&mut app, owner, "previous");
    assert_eq!(focused_name(&app), "z");
}

#[test]
fn focused_activation_and_pointer_share_current_value_and_availability() {
    let (mut app, owner) = fixture(
        r#"<node bind:disabled="disabled" bind:visible="shown"><button id="choice" on-click="choose" bind:value="value"/></node>"#,
    );
    input(&mut app, owner, "next");
    let button = focused(&app).unwrap();
    input(&mut app, owner, "change");
    input(&mut app, owner, "activate");
    app.update();
    assert_eq!(received(&app, owner), "menu:choose:new");
    click(&mut app, button, PointerButton::Primary);
    app.update();
    assert_eq!(number(&app, owner, "actions"), 2);
    input(&mut app, owner, "disable");
    input(&mut app, owner, "activate");
    click(&mut app, button, PointerButton::Primary);
    app.update();
    assert_eq!(number(&app, owner, "actions"), 2);
    input(&mut app, owner, "disable");
    input(&mut app, owner, "hide");
    input(&mut app, owner, "activate");
    app.update();
    assert_eq!(number(&app, owner, "actions"), 2);
    input(&mut app, owner, "hide");
    input(&mut app, owner, "invalid");
    input(&mut app, owner, "activate");
    app.update();
    assert_eq!(number(&app, owner, "actions"), 2);
}

#[test]
fn navigation_is_scoped_to_one_activity_and_one_controller() {
    let (mut app, first) = fixture(r#"<button id="choice" on-click="choose"/>"#);
    let second = spawn(&mut app);
    wait_for(&mut app, |app| number(app, second, "ready_count") == 2);
    input(&mut app, first, "other_next");
    assert_eq!(focused_name(&app), "other-button");
    input(&mut app, first, "activate");
    app.update();
    assert_eq!(
        number(&app, first, "actions"),
        0,
        "do not activate another scope"
    );
    input(&mut app, second, "next");
    input(&mut app, second, "activate");
    app.update();
    assert_eq!(number(&app, first, "actions"), 0);
    assert_eq!(received(&app, second), "menu:choose:nil");
}

#[test]
fn readiness_is_once_per_mount_and_settled_mounts_leave_the_pending_query() {
    let (mut app, owner) = fixture(r#"<button on-click="choose"/>"#);
    for _ in 0..20 {
        app.update();
    }
    assert_eq!(number(&app, owner, "ready_count"), 2);
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<PendingReady>>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(app.world().resource::<ViewIndex>().0.len(), 2);
    app.world_mut().despawn(owner);
    assert!(app.world().resource::<ViewIndex>().0.is_empty());
}

#[test]
fn cancelled_generation_cannot_activate_its_replacement_and_does_not_leak_index_entries() {
    let (mut app, owner) = fixture(r#"<button id="choice" on-click="choose"/>"#);
    input(&mut app, owner, "next");
    queue(&mut app, owner, "activate");
    queue(&mut app, owner, "restart");
    app.update();
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(number(&app, owner, "actions"), 0);
    assert_eq!(
        app.world()
            .get::<StateTreeInstance>(owner)
            .unwrap()
            .status(),
        &InstanceStatus::Running
    );
    for _ in 0..20 {
        input(&mut app, owner, "restart");
        app.update();
    }
    assert_eq!(app.world().resource::<ViewIndex>().0.len(), 2);
    queue(&mut app, owner, "next");
    queue(&mut app, owner, "restart");
    app.update();
    assert_eq!(number(&app, owner, "actions"), 0);
}

#[test]
fn repeated_rows_reorder_cached_navigation_without_losing_row_identity() {
    let (mut app, owner) = fixture(
        r#"<node><repeat bind:items="rows" as="row" key="id"><button on-click="choose" bind:value="row.id"/></repeat></node>"#,
    );
    input(&mut app, owner, "next");
    let oak = focused(&app).unwrap();
    input(&mut app, owner, "next");
    let ash = focused(&app).unwrap();
    input(&mut app, owner, "reorder");
    input(&mut app, owner, "next");
    assert_eq!(focused(&app), Some(oak));
    input(&mut app, owner, "previous");
    assert_eq!(focused(&app), Some(ash));
    input(&mut app, owner, "activate");
    app.update();
    assert_eq!(received(&app, owner), "menu:choose:ash");
    input(&mut app, owner, "remove");
    input(&mut app, owner, "activate");
    app.update();
    assert_eq!(number(&app, owner, "actions"), 1);
}

#[test]
fn malformed_tab_indices_are_rejected() {
    for index in ["-2", "1.5", "first", "2147483648"] {
        assert!(
            crate::source::parse(
                "bad.kui",
                &format!(r#"{HEADER}<button tab-index="{index}"/></view>"#)
            )
            .is_err()
        );
    }
}
