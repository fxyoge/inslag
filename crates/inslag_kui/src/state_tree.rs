//! Render views and handle their interactions from SCXML.
//!
//! [`KuiStateTreePlugin`] adds these activities and effects:
//!
//! | Kind | Name | Arguments |
//! | --- | --- | --- |
//! | Activity | `ui.render` | `src`: `.kui` asset path; `space`: `layout` (default) or `world` |
//! | Effect | `ui.focus` | `view`: activity ID; `direction`: `next` or `previous` |
//! | Effect | `ui.activate` | `view`: activity ID |
//!
//! Target focus and activation with the ID of an active `ui.render` activity:
//!
//! ```xml
//! <transition event="ui.render.ready">
//!   <ist:effect name="ui.focus">
//!     <ist:arg name="view" expr="'menu'"/>
//!     <ist:arg name="direction" expr="'next'"/>
//!   </ist:effect>
//! </transition>
//! ```
//!
//! Declare `xmlns:ist="urn:inslag-state-tree"` on the chart's root.
//! Buttons send `ui.action` with `view`, `action`, `value`, `position`, and `start`
//! in `_event.data`. Use `ui.render.ready` to set initial focus and
//! `ui.focus.changed` to react to focus changes; both include `view`.
mod plugins;
pub use plugins::KuiStateTreePlugin;

use std::collections::BTreeMap;

use bevy::prelude::*;
use inslag_state_tree::{
    EntityStateTreeEvent, EntityStateTreeEventWriter, InstanceStatus, LuauValue, StateTreeActivity,
    StateTreeActivityLifecycle, StateTreeActivityOutcomeWriter, StateTreeActivityToken,
    StateTreeEvent, StateTreeExt, StateTreeInput, StateTreeInstance, StateTreeSet,
};

use crate::{
    KuiAction, KuiAsset, KuiBinding, KuiNode, KuiNodeKind, KuiPlugin, KuiSystems, KuiValue,
    KuiView, KuiViewStatus, validate_asset_path,
};

#[derive(Clone, Debug, StateTreeInput)]
struct UiRender {
    #[state_tree(activity_id)]
    id: String,
    pub src: String,
    #[state_tree(default)]
    space: RenderSpace,
}

#[derive(Clone, Copy, Debug, Default, StateTreeInput)]
#[state_tree(rename_all = "snake_case")]
enum RenderSpace {
    #[default]
    Layout,
    World,
}

impl StateTreeActivity for UiRender {
    type Output = ();
    type Error = String;

    fn error_data(error: &Self::Error) -> Option<LuauValue> {
        Some(LuauValue::String(error.clone()))
    }
}

#[derive(Clone, Debug, EntityEvent)]
struct UiAction {
    pub entity: Entity,
    pub action: String,
    pub value: Option<String>,
    pub data: crate::KuiActionData,
    pub view: String,
}

impl StateTreeEvent for UiAction {
    const AUTHORED_NAME: &'static str = "ui.action";

    fn event_data(&self) -> Option<LuauValue> {
        Some(LuauValue::Record(BTreeMap::from([
            ("view".into(), LuauValue::String(self.view.clone())),
            ("action".into(), LuauValue::String(self.action.clone())),
            (
                "value".into(),
                self.value
                    .as_ref()
                    .map_or(LuauValue::Nil, |value| LuauValue::String(value.clone())),
            ),
            ("position".into(), project_position(self.data.position)),
            ("start".into(), project_position(self.data.start)),
        ])))
    }
}
impl EntityStateTreeEvent for UiAction {}

#[derive(Clone, Debug, EntityEvent, inslag_state_tree::StateTreeEvent)]
#[state_tree(name = "ui.render.ready", data = view_event_data)]
struct UiReady {
    entity: Entity,
    view: String,
}

fn view_event_data(event: &UiReady) -> LuauValue {
    LuauValue::Record(BTreeMap::from([(
        "view".into(),
        LuauValue::String(event.view.clone()),
    )]))
}

#[derive(Clone, Debug, EntityEvent, inslag_state_tree::StateTreeEvent)]
#[state_tree(name = "ui.focus.changed", data = focus_event_data)]
struct UiFocusChanged {
    entity: Entity,
    view: String,
}

fn focus_event_data(event: &UiFocusChanged) -> LuauValue {
    LuauValue::Record(BTreeMap::from([(
        "view".into(),
        LuauValue::String(event.view.clone()),
    )]))
}

#[derive(Clone, Copy, StateTreeInput)]
#[state_tree(rename_all = "snake_case")]
enum Direction {
    Next,
    Previous,
}

#[derive(Message, StateTreeInput)]
struct FocusView {
    #[state_tree(owner)]
    owner: Entity,
    #[state_tree(activity = "view")]
    token: Option<StateTreeActivityToken<UiRender>>,
    direction: Direction,
}

#[derive(Message, StateTreeInput)]
struct ActivateView {
    #[state_tree(owner)]
    owner: Entity,
    #[state_tree(activity = "view")]
    token: Option<StateTreeActivityToken<UiRender>>,
}

fn project_position(position: Option<Vec2>) -> LuauValue {
    position.map_or(LuauValue::Nil, |position| {
        LuauValue::Record(BTreeMap::from([
            ("x".into(), LuauValue::Number(position.x as f64)),
            ("y".into(), LuauValue::Number(position.y as f64)),
        ]))
    })
}

#[derive(Component)]
struct StateTreeView {
    id: String,
    token: StateTreeActivityToken<UiRender>,
    sampled_asset: Option<AssetId<KuiAsset>>,
    input_roots: Option<Vec<String>>,
}

#[derive(Resource, Default)]
struct ViewIndex(std::collections::HashMap<StateTreeActivityToken<UiRender>, Entity>);

#[derive(Component)]
struct PendingReady;

#[derive(Component)]
struct FocusScope {
    dirty: bool,
    ordered: Vec<(i32, Entity)>,
    cursor: Option<(Entity, usize)>,
}

pub(crate) fn invalidate_view(commands: &mut Commands, view: Entity) {
    commands.entity(view).insert(PendingReady);
    invalidate_focus(commands, view);
}

pub(crate) fn invalidate_focus(commands: &mut Commands, view: Entity) {
    commands
        .entity(view)
        .entry::<FocusScope>()
        .and_modify(|mut scope| scope.dirty = true);
}

fn manage_activities(
    mut lifecycle: MessageReader<StateTreeActivityLifecycle<UiRender>>,
    asset_server: Res<AssetServer>,
    mut commands: Commands,
    mut outcomes: StateTreeActivityOutcomeWriter<UiRender>,
) {
    for lifecycle in lifecycle.read() {
        match lifecycle {
            StateTreeActivityLifecycle::Started { token, activity } => {
                if let Err(error) = validate_asset_path(&activity.src) {
                    outcomes.fail(*token, error.into());
                    continue;
                }
                let token = *token;
                let asset = asset_server.load::<KuiAsset>(activity.src.clone());
                let world_space = matches!(activity.space, RenderSpace::World);
                let id = activity.id.clone();
                commands.queue(move |world: &mut World| {
                    if world
                        .get::<StateTreeInstance>(token.entity())
                        .is_some_and(|tree| tree.is_activity_active(token))
                    {
                        let mut view = KuiView::new(token.entity(), asset);
                        view.world_space = world_space;
                        let mut mount = world.spawn((
                            view,
                            StateTreeView {
                                id,
                                token,
                                sampled_asset: None,
                                input_roots: None,
                            },
                            PendingReady,
                            FocusScope {
                                dirty: true,
                                ordered: Vec::new(),
                                cursor: None,
                            },
                        ));
                        if world_space {
                            mount.insert((Transform::default(), Visibility::default()));
                        }
                        let entity = mount.id();
                        world.resource_mut::<ViewIndex>().0.insert(token, entity);
                    }
                });
            }
            StateTreeActivityLifecycle::Cancelled { token } => {
                let token = *token;
                // Apply lifecycle operations in message order, including a start
                // and cancellation produced during the same state-tree update.
                commands.queue(move |world: &mut World| {
                    if let Some(mount) = world.resource_mut::<ViewIndex>().0.remove(&token) {
                        world.despawn(mount);
                    }
                });
            }
        }
    }
}

fn remove_view(
    event: On<Remove<StateTreeView>>,
    views: Query<&StateTreeView>,
    mut index: ResMut<ViewIndex>,
) {
    if let Ok(view) = views.get(event.entity) {
        index.0.remove(&view.token);
    }
}

#[cfg_attr(
    dylint_lib = "inslag_state_tree_private_access",
    allow(
        inslag_state_tree_private_access,
        reason = "the generic KUI adapter supplies explicitly authored view bindings"
    )
)]
fn supply_data(
    mut views: Query<(&mut StateTreeView, &mut KuiView)>,
    controllers: Query<Ref<StateTreeInstance>>,
    assets: Res<Assets<KuiAsset>>,
) {
    for (mut mount, mut view) in &mut views {
        let Ok(instance) = controllers.get(mount.token.entity()) else {
            continue;
        };
        if !matches!(
            instance.status(),
            InstanceStatus::Running | InstanceStatus::Complete
        ) {
            continue;
        }
        let Some(asset) = assets.get(&view.asset) else {
            continue;
        };
        if mount.sampled_asset == Some(view.asset.id())
            && !instance.is_changed()
            && !assets.is_changed()
        {
            continue;
        }
        let mut data = BTreeMap::new();
        let asset_changed = mount.sampled_asset != Some(view.asset.id()) || assets.is_changed();
        if asset_changed {
            mount.input_roots = asset
                .definition
                .children
                .iter()
                .try_fold(std::collections::BTreeSet::new(), |mut roots, node| {
                    roots.extend(crate::path::external_roots(node, &[])?);
                    Some(roots)
                })
                .map(|roots| roots.into_iter().collect());
        }
        if let Some(roots) = &mount.input_roots {
            // A controller event need not change this view's inputs. Compare
            // borrowed roots before allocating/cloning a replacement data map.
            if !asset_changed
                && roots
                    .iter()
                    .all(|name| view.data.get(name) == instance.data(name))
            {
                continue;
            }
            for name in roots {
                if let Some(value) = instance.data(name) {
                    data.insert(name.clone(), value.clone());
                }
            }
        } else {
            // Conservatively handle a caller that crosses the analyzed scope.
            collect_data(&asset.definition.children, &instance, &mut data);
        }
        if view.data != data {
            view.data = data;
        }
        mount.sampled_asset = Some(view.asset.id());
    }
}

fn collect_data(
    nodes: &[KuiNode],
    instance: &StateTreeInstance,
    data: &mut BTreeMap<String, KuiValue>,
) {
    collect_scoped_data(nodes, instance, data, &[]);
}

fn collect_scoped_data(
    nodes: &[KuiNode],
    instance: &StateTreeInstance,
    data: &mut BTreeMap<String, KuiValue>,
    aliases: &[String],
) {
    collect_component_data(nodes, instance, data, aliases, &[]);
}

fn collect_component_data(
    nodes: &[KuiNode],
    instance: &StateTreeInstance,
    data: &mut BTreeMap<String, KuiValue>,
    aliases: &[String],
    callers: &[Vec<String>],
) {
    for node in nodes {
        if let KuiNodeKind::Repeat {
            items,
            alias,
            template,
            ..
        } = &node.kind
        {
            if callers.is_empty() {
                collect_path_data(items, instance, data, aliases);
            }
            let mut nested = aliases.to_vec();
            nested.push(alias.clone());
            collect_component_data(
                std::slice::from_ref(template),
                instance,
                data,
                &nested,
                callers,
            );
            continue;
        }
        let mut paths: Vec<_> = node.bindings.iter().flat_map(KuiBinding::paths).collect();
        if let KuiNodeKind::Scope { inputs, .. } = &node.kind {
            paths.extend(inputs.values().filter_map(|value| match value {
                crate::KuiPropSource::Binding(path) => Some(path.as_str()),
                _ => None,
            }));
        }
        if let KuiNodeKind::Meter { current, maximum } = &node.kind {
            paths.extend([current.as_str(), maximum.as_str()]);
        }
        for path in paths {
            if callers.is_empty() {
                collect_path_data(path, instance, data, aliases);
            }
        }
        if let KuiNodeKind::Node { children } | KuiNodeKind::Button { children, .. } = &node.kind {
            collect_component_data(children, instance, data, aliases, callers);
        }
        match &node.kind {
            KuiNodeKind::Scope { template, .. } => {
                let mut stack = callers.to_vec();
                stack.push(aliases.to_vec());
                collect_component_data(
                    std::slice::from_ref(template),
                    instance,
                    data,
                    &["props".into()],
                    &stack,
                );
            }
            KuiNodeKind::Caller { children, .. } => {
                let caller = callers.last().expect("compiled caller scope");
                collect_component_data(
                    children,
                    instance,
                    data,
                    caller,
                    &callers[..callers.len() - 1],
                );
            }
            KuiNodeKind::Slot { children, .. } => {
                collect_component_data(children, instance, data, aliases, callers)
            }
            _ => {}
        }
    }
}

#[cfg_attr(
    dylint_lib = "inslag_state_tree_private_access",
    allow(
        inslag_state_tree_private_access,
        reason = "the generic KUI adapter resolves explicitly authored binding roots"
    )
)]
fn collect_path_data(
    path: &str,
    instance: &StateTreeInstance,
    data: &mut BTreeMap<String, KuiValue>,
    aliases: &[String],
) {
    for name in crate::expression::roots(path) {
        if !aliases.contains(&name)
            && !data.contains_key(&name)
            && let Some(value) = instance.data(&name)
        {
            data.insert(name, value.clone());
        }
    }
}

fn report_failures(
    views: Query<(Entity, &StateTreeView, &KuiView), Changed<KuiView>>,
    mut outcomes: StateTreeActivityOutcomeWriter<UiRender>,
    mut commands: Commands,
    mut index: ResMut<ViewIndex>,
) {
    for (entity, mount, view) in &views {
        if let KuiViewStatus::Failed(message) = view.status() {
            index.0.remove(&mount.token);
            outcomes.fail(mount.token, message.clone());
            commands.entity(entity).despawn();
        }
    }
}

fn forward_action(
    action: On<KuiAction>,
    views: Query<&StateTreeView>,
    controllers: Query<&StateTreeInstance>,
    mut events: EntityStateTreeEventWriter,
) {
    let Ok(view) = views.get(action.view) else {
        return;
    };
    if action.entity == view.token.entity()
        && controllers
            .get(action.entity)
            .is_ok_and(|tree| tree.is_activity_active(view.token))
    {
        events.write_for_activity(
            view.token,
            UiAction {
                entity: action.entity,
                action: action.action.clone(),
                value: action.value.clone(),
                data: action.data,
                view: view.id.clone(),
            },
        );
    }
}

fn report_ready(
    views: Query<(Entity, &StateTreeView, &KuiView), With<PendingReady>>,
    controllers: Query<&StateTreeInstance>,
    assets: Res<AssetServer>,
    mut events: EntityStateTreeEventWriter,
    mut commands: Commands,
) {
    for (entity, mount, view) in &views {
        if view.status() == &KuiViewStatus::Ready
            && assets.is_loaded_with_dependencies(view.asset.id())
            && controllers
                .get(mount.token.entity())
                .is_ok_and(|tree| tree.is_activity_active(mount.token))
        {
            events.write_for_activity(
                mount.token,
                UiReady {
                    entity: mount.token.entity(),
                    view: mount.id.clone(),
                },
            );
            commands.entity(entity).remove::<PendingReady>();
        }
    }
}

fn focus_view(
    mut requests: MessageReader<FocusView>,
    index: Res<ViewIndex>,
    mut views: Query<(&StateTreeView, &KuiView, &mut FocusScope)>,
    controllers: Query<&StateTreeInstance>,
    children: Query<&Children>,
    buttons: Query<(&crate::runtime::TabIndex, &crate::runtime::ActionBindings)>,
    actions: crate::interaction::Actions,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    mut visible: ResMut<bevy::input_focus::InputFocusVisible>,
    mut events: EntityStateTreeEventWriter,
) {
    for request in requests.read() {
        let Some(token) = request.token else {
            warn!(owner = ?request.owner, "UI navigation: view activity is not active");
            continue;
        };
        if !controllers
            .get(token.entity())
            .is_ok_and(|tree| tree.is_activity_active(token))
        {
            continue;
        }
        let Some(entity) = index.0.get(&token).copied() else {
            warn!(owner = ?request.owner, "UI navigation: view is unavailable");
            continue;
        };
        let Ok((mount, view, mut scope)) = views.get_mut(entity) else {
            continue;
        };
        if view.status() != &KuiViewStatus::Ready {
            warn!(owner = ?request.owner, "UI navigation: view is not ready");
            continue;
        }
        if scope.dirty {
            scope.ordered.clear();
            scope.cursor = None;
            let mut stack: Vec<_> = children
                .get(entity)
                .map(|children| children.iter().rev().collect())
                .unwrap_or_default();
            while let Some(entity) = stack.pop() {
                if let Ok((index, binding)) = buttons.get(entity)
                    && index.0 >= 0
                    && binding.events.contains_key("on-click")
                {
                    scope.ordered.push((index.0, entity));
                }
                if let Ok(children) = children.get(entity) {
                    stack.extend(children.iter().rev());
                }
            }
            scope.ordered.sort_by_key(|(index, _)| {
                if *index == 0 {
                    i64::MAX
                } else {
                    i64::from(*index)
                }
            });
            scope.dirty = false;
        }
        let count = scope.ordered.len();
        if count == 0 {
            continue;
        }
        let current = focus.get().and_then(|focused| {
            scope
                .cursor
                .filter(|(entity, _)| *entity == focused)
                .map(|(_, index)| index)
                .or_else(|| {
                    scope
                        .ordered
                        .iter()
                        .position(|(_, entity)| *entity == focused)
                })
        });
        // Check availability lazily in navigation order; a warmed next/previous
        // request visits only skipped unavailable controls and its destination.
        let destination = (1..=count).find_map(|step| {
            let index = match (current, request.direction) {
                (Some(index), Direction::Previous) => (index + count - step) % count,
                (Some(index), _) => (index + step) % count,
                (None, Direction::Previous) => count - step,
                (None, _) => step - 1,
            };
            let entity = scope.ordered[index].1;
            actions.enabled(entity).then_some((entity, index))
        });
        let Some((target, index)) = destination else {
            continue;
        };
        scope.cursor = Some((target, index));
        if Some(target) != focus.get() {
            focus.set(target, bevy::input_focus::FocusCause::Navigated);
            visible.0 = true;
            events.write_for_activity(
                token,
                UiFocusChanged {
                    entity: token.entity(),
                    view: mount.id.clone(),
                },
            );
        }
    }
}

fn activate_view(
    mut requests: MessageReader<ActivateView>,
    index: Res<ViewIndex>,
    views: Query<&KuiView>,
    controllers: Query<&StateTreeInstance>,
    buttons: Query<(&crate::runtime::TabIndex, &crate::runtime::ActionBindings)>,
    actions: crate::interaction::Actions,
    focus: Res<bevy::input_focus::InputFocus>,
    mut commands: Commands,
) {
    for request in requests.read() {
        let Some(token) = request.token else {
            warn!(owner = ?request.owner, "UI navigation: view activity is not active");
            continue;
        };
        if !controllers
            .get(token.entity())
            .is_ok_and(|tree| tree.is_activity_active(token))
        {
            continue;
        }
        let Some(entity) = index.0.get(&token).copied() else {
            warn!(owner = ?request.owner, "UI navigation: view is unavailable");
            continue;
        };
        let Ok(view) = views.get(entity) else {
            continue;
        };
        if view.status() != &KuiViewStatus::Ready {
            warn!(owner = ?request.owner, "UI navigation: view is not ready");
            continue;
        }
        // Stale focus must not activate a different available control.
        if let Some(target) = focus.get()
            && let Ok((index, binding)) = buttons.get(target)
            && index.0 >= 0
            && binding.view == entity
            && binding.events.contains_key("on-click")
            && actions.enabled(target)
        {
            actions.emit(&mut commands, target, "on-click", default());
        }
    }
}

#[cfg(test)]
mod navigation_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        KuiViewRoot,
        runtime::test_support,
        tests::{HEADER, click, wait_for},
    };
    use bevy::ecs::system::RunSystemOnce;
    use bevy::ui_widgets::Button;
    use inslag_state_tree::{StateTreeDefinition, StateTreePlugin, scxml};

    fn spawn(app: &mut App, path: &str) -> Entity {
        let definition = definition(path);
        let handle = app
            .world_mut()
            .resource_mut::<Assets<StateTreeDefinition>>()
            .add(definition);
        app.world_mut().spawn(StateTreeInstance::new(handle)).id()
    }

    fn definition(path: &str) -> StateTreeDefinition {
        let source = format!(
            r#"
          <scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:bst="urn:inslag-state-tree" datamodel="luau" initial="active">
            <datamodel>
              <data id="label" expr="'ready'"/>
              <data id="disabled" expr="false"/>
              <data id="received" expr="'none'"/>
              <data id="rows" expr="{{{{id = 'oak', title = 'Oak'}}}}"/>
              <data id="labels" expr="{{oak = 'First', pine = 'Second'}}"/>
              <data id="chosen" expr="'oak'"/>
            </datamodel>
            <state id="active">
              <bst:activity id="view" name="ui.render"><bst:arg name="src" expr="'{path}'"/></bst:activity>
              <transition event="ui.action" cond="_event.data.action == 'close'" target="inactive"/>
              <transition event="ui.action" cond="_event.data.action == 'retarget'"><assign location="chosen" expr="'pine'"/></transition>
              <transition event="ui.action" cond="_event.data.action == 'patch'"><assign location="labels[chosen]" expr="'Updated'"/></transition>
              <transition event="ui.action"><assign location="received" expr="_event.data.action .. ':' .. (_event.data.value or 'nil')"/></transition>
              <transition event="error.activity.view" target="failed"><assign location="label" expr="'failed'"/></transition>
            </state>
            <state id="inactive"/>
            <state id="failed"/>
          </scxml>"#,
        );
        scxml::compile(&scxml::parse("test.scxml", &source).unwrap()).unwrap()
    }

    fn data(app: &App, entity: Entity, name: &str) -> String {
        let LuauValue::String(value) = app
            .world()
            .get::<StateTreeInstance>(entity)
            .unwrap()
            .data(name)
            .unwrap()
        else {
            panic!()
        };
        value.to_string()
    }

    fn send(app: &mut App, entity: Entity, action: &str) {
        let action = action.to_owned();
        app.world_mut()
            .run_system_once(move |mut events: EntityStateTreeEventWriter| {
                events.write(UiAction {
                    view: "view".into(),
                    entity,
                    action: action.clone(),
                    value: None,
                    data: default(),
                });
            })
            .unwrap();
    }

    #[test]
    fn cached_input_roots_refresh_after_same_handle_asset_edits() {
        let source = format!(r#"{HEADER}<node><text bind:text="label"/></node></view>"#);
        let mut app = test_support::app(&[("cached.kui", &source)]);
        app.add_plugins((StateTreePlugin::new(Update), KuiStateTreePlugin));
        let owner = spawn(&mut app, "cached.kui");
        wait_for(&mut app, |app| {
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .any(|text| text.0 == "ready")
        });
        let handle = app
            .world_mut()
            .query::<&KuiView>()
            .single(app.world())
            .unwrap()
            .asset
            .clone();
        let changed = format!(r#"{HEADER}<node><text bind:text="received"/></node></view>"#);
        app.world_mut()
            .resource_mut::<Assets<KuiAsset>>()
            .get_mut(&handle)
            .unwrap()
            .definition = crate::parse("cached.kui", &changed).unwrap();
        wait_for(&mut app, |app| {
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .any(|text| text.0 == "none")
        });
        let view = app
            .world_mut()
            .query::<&KuiView>()
            .single(app.world())
            .unwrap();
        assert!(view.data.contains_key("received"));
        assert!(!view.data.contains_key("label"));
        send(&mut app, owner, "cached");
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&Text>()
                .single(app.world())
                .unwrap()
                .0,
            "cached:nil"
        );
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn checkpoint_reconstructs_views_from_datamodel_and_discards_pending_actions() {
        use inslag_state_tree::persistence::Checkpoint;
        let source = format!(r#"{HEADER}<node><text bind:text="received"/></node></view>"#);
        let mut app = test_support::app(&[("saved.kui", &source)]);
        app.add_plugins((StateTreePlugin::new(Update), KuiStateTreePlugin));
        let owner = spawn(&mut app, "saved.kui");
        app.world_mut()
            .entity_mut(owner)
            .insert(inslag_state_tree::PersistStateTree);
        wait_for(&mut app, |app| {
            app.world_mut()
                .query::<&KuiView>()
                .iter(app.world())
                .any(|view| view.status() == &KuiViewStatus::Ready)
        });
        send(&mut app, owner, "saved-action");
        app.update();
        send(&mut app, owner, "discarded-action");
        let checkpoint = Checkpoint::capture(app.world_mut()).unwrap();
        checkpoint.restore(app.world_mut()).unwrap();
        let owner = app
            .world_mut()
            .query::<(Entity, &StateTreeInstance)>()
            .single(app.world())
            .unwrap()
            .0;
        wait_for(&mut app, |app| {
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .any(|text| text.0 == "saved-action:nil")
        });
        assert_eq!(data(&app, owner, "received"), "saved-action:nil");
        assert_eq!(
            app.world_mut()
                .query::<&StateTreeView>()
                .iter(app.world())
                .count(),
            1
        );
        send(&mut app, owner, "close");
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&StateTreeView>()
                .iter(app.world())
                .count(),
            0
        );
    }

    #[test]
    fn keyed_paths_sample_key_dependencies_and_retarget_through_controller_data() {
        let source = format!(
            r#"{HEADER}<node><text id="lookup" bind:text="labels[chosen]"/><repeat bind:items="rows" as="row" key="id"><text template="{{labels[row.id]}}"/></repeat></node></view>"#
        );
        let mut app = test_support::app(&[("lookup.kui", &source)]);
        app.add_plugins((StateTreePlugin::new(Update), KuiStateTreePlugin));
        let owner = spawn(&mut app, "lookup.kui");
        wait_for(&mut app, |app| {
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .any(|text| text.0 == "First")
        });
        let view = app
            .world_mut()
            .query::<&KuiView>()
            .single(app.world())
            .unwrap();
        assert!(view.data.contains_key("labels"));
        assert!(view.data.contains_key("chosen"));
        assert!(!view.data.contains_key("row"));
        send(&mut app, owner, "retarget");
        app.update();
        let mut texts = app
            .world_mut()
            .query::<&Text>()
            .iter(app.world())
            .map(|text| text.0.clone())
            .collect::<Vec<_>>();
        texts.sort();
        assert_eq!(texts, ["First", "Second"]);
        send(&mut app, owner, "patch");
        app.update();
        let mut texts = app
            .world_mut()
            .query::<&Text>()
            .iter(app.world())
            .map(|text| text.0.clone())
            .collect::<Vec<_>>();
        texts.sort();
        assert_eq!(texts, ["First", "Updated"]);
    }

    #[test]
    fn sequence_bindings_and_row_payloads_work_through_state_tree_data() {
        let source = format!(
            r#"{HEADER}<node><repeat bind:items="rows" as="row" key="id"><button on-click="choose" bind:value="row.id"><text bind:text="row.title"/><text template="{{label}} · {{disabled}}"/></button></repeat></node></view>"#
        );
        let mut app = test_support::app(&[("rows.kui", &source)]);
        app.add_plugins((StateTreePlugin::new(Update), KuiStateTreePlugin));
        let owner = spawn(&mut app, "rows.kui");
        wait_for(&mut app, |app| {
            app.world_mut()
                .query::<&crate::KuiRepeatItem>()
                .iter(app.world())
                .count()
                == 1
        });
        let row = app
            .world_mut()
            .query::<(Entity, &crate::KuiRepeatItem)>()
            .iter(app.world())
            .next()
            .unwrap()
            .0;
        let children = app.world().get::<Children>(row).unwrap();
        assert_eq!(app.world().get::<Text>(children[0]).unwrap().0, "Oak");
        assert_eq!(
            app.world().get::<Text>(children[1]).unwrap().0,
            "ready · false"
        );
        let view = app
            .world_mut()
            .query::<&KuiView>()
            .iter(app.world())
            .next()
            .unwrap();
        assert!(view.data.contains_key("rows"));
        assert!(view.data.contains_key("label"));
        assert!(view.data.contains_key("disabled"));
        assert!(!view.data.contains_key("row"));
        click(&mut app, row, PointerButton::Primary);
        app.update();
        assert_eq!(data(&app, owner, "received"), "choose:oak");
    }

    #[test]
    fn component_inputs_and_caller_slots_sample_controller_data_and_forward_events() {
        let source = format!(
            r#"{HEADER}<import name="choice" src="choice.kui"/><node><repeat bind:items="rows" as="row" key="id"><use component="choice"><prop name="title" bind:value="row.title"/><prop name="id" bind:value="row.id"/><prop name="disabled" bind:value="disabled"/><on name="chosen" action="choose"/><fill slot="detail"><text template="{{label}} · {{row.title}}"/></fill></use></repeat></node></view>"#
        );
        let component = r#"<component xmlns="urn:inslag-kui:1" xmlns:bind="urn:inslag-kui:binding:1" version="1" name="choice"><prop name="title" type="string"/><prop name="id" type="scalar"/><prop name="disabled" type="boolean"/><event name="chosen"/><button on-click="emit.chosen" bind:value="props.id" bind:disabled="props.disabled"><text bind:text="props.title"/><slot name="detail"/></button></component>"#;
        let mut app = test_support::app(&[("rows.kui", &source), ("choice.kui", component)]);
        app.add_plugins((StateTreePlugin::new(Update), KuiStateTreePlugin));
        let owner = spawn(&mut app, "rows.kui");
        wait_for(&mut app, |app| {
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .any(|text| text.0 == "ready · Oak")
        });
        let button = app
            .world_mut()
            .query_filtered::<Entity, With<Button>>()
            .single(app.world())
            .unwrap();
        let view = app
            .world_mut()
            .query::<&KuiView>()
            .single(app.world())
            .unwrap();
        assert_eq!(
            view.data.keys().map(String::as_str).collect::<Vec<_>>(),
            ["disabled", "label", "rows"]
        );
        click(&mut app, button, PointerButton::Primary);
        app.update();
        assert_eq!(data(&app, owner, "received"), "choose:oak");
    }

    #[test]
    fn mounts_syncs_dispatches_and_cancels_independent_controller_views() {
        let source = format!(
            r#"{HEADER}<button on-click="choose" value="oak" bind:disabled="disabled"><text bind:text="label">waiting</text></button></view>"#
        );
        let mut app = test_support::app(&[("test.kui", &source)]);
        app.add_plugins((StateTreePlugin::new(Update), KuiStateTreePlugin));
        let first = spawn(&mut app, "test.kui");
        let second = spawn(&mut app, "test.kui");
        wait_for(&mut app, |app| {
            app.world_mut()
                .query::<&KuiViewRoot>()
                .iter(app.world())
                .count()
                == 2
        });
        let button = app
            .world_mut()
            .query::<(Entity, &KuiViewRoot)>()
            .iter(app.world())
            .find_map(|(entity, root)| (root.controller == first).then_some(entity))
            .unwrap();
        let text = app.world().get::<Children>(button).unwrap()[0];
        assert_eq!(app.world().get::<Text>(text).unwrap().0, "ready");
        click(&mut app, button, PointerButton::Primary);
        app.update();
        assert_eq!(data(&app, first, "received"), "choose:oak");
        assert_eq!(data(&app, second, "received"), "none");
        send(&mut app, first, "close");
        app.update();
        assert!(app.world().get_entity(button).is_err());
        assert_eq!(
            app.world_mut()
                .query::<&KuiViewRoot>()
                .iter(app.world())
                .count(),
            1
        );
        app.world_mut()
            .entity_mut(second)
            .remove::<StateTreeInstance>();
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&KuiViewRoot>()
                .iter(app.world())
                .count(),
            0
        );
        assert_eq!(
            app.world_mut()
                .query::<&KuiView>()
                .iter(app.world())
                .count(),
            0
        );
    }

    #[test]
    fn replacing_a_tree_replaces_its_views_and_preserves_owner_routing() {
        let source = format!(
            r#"{HEADER}<button on-click="choose" value="oak"><text bind:text="label"/></button></view>"#
        );
        let mut app = test_support::app(&[("test.kui", &source)]);
        app.add_plugins((StateTreePlugin::new(Update), KuiStateTreePlugin));
        let owner = spawn(&mut app, "test.kui");
        wait_for(&mut app, |app| {
            app.world_mut()
                .query::<&KuiViewRoot>()
                .iter(app.world())
                .count()
                == 1
        });
        let old_root = app
            .world_mut()
            .query_filtered::<Entity, With<KuiViewRoot>>()
            .single(app.world())
            .unwrap();
        let second_def = definition("test.kui");
        let handle = app
            .world_mut()
            .resource_mut::<Assets<StateTreeDefinition>>()
            .add(second_def);
        app.world_mut()
            .entity_mut(owner)
            .insert(StateTreeInstance::new(handle));
        wait_for(&mut app, |app| {
            app.world_mut()
                .query::<&KuiViewRoot>()
                .iter(app.world())
                .count()
                == 1
                && app.world().get_entity(old_root).is_err()
        });
        let first_view = app
            .world_mut()
            .query_filtered::<Entity, With<StateTreeView>>()
            .single(app.world())
            .unwrap();
        let root = app.world().get::<KuiView>(first_view).unwrap().roots()[0];
        click(&mut app, root, PointerButton::Primary);
        app.update();
        assert_eq!(data(&app, owner, "received"), "choose:oak");
        send(&mut app, owner, "close");
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&KuiViewRoot>()
                .iter(app.world())
                .count(),
            0
        );
    }

    #[test]
    fn asset_failure_reports_activity_outcome_and_unmounts() {
        let mut app = test_support::app(&[]);
        app.add_plugins((StateTreePlugin::new(Update), KuiStateTreePlugin));
        let owner = spawn(&mut app, "missing.kui");
        wait_for(&mut app, |app| data(app, owner, "label") == "failed");
        assert_eq!(
            app.world_mut()
                .query::<&KuiView>()
                .iter(app.world())
                .count(),
            0
        );
        assert_eq!(
            app.world_mut()
                .query::<&KuiViewRoot>()
                .iter(app.world())
                .count(),
            0
        );
    }

    #[test]
    fn action_projection_preserves_optional_value() {
        for (value, expected) in [
            (Some("oak".to_owned()), LuauValue::String("oak".into())),
            (None, LuauValue::Nil),
        ] {
            let action = UiAction {
                view: "view".into(),
                entity: Entity::PLACEHOLDER,
                action: "choose".into(),
                value,
                data: default(),
            };
            assert_eq!(
                <UiAction as StateTreeEvent>::event_data(&action),
                Some(LuauValue::Record(BTreeMap::from([
                    ("view".into(), LuauValue::String("view".into())),
                    ("action".into(), LuauValue::String("choose".into())),
                    ("value".into(), expected),
                    ("position".into(), LuauValue::Nil),
                    ("start".into(), LuauValue::Nil),
                ])))
            );
        }
    }

    #[derive(Resource, Default)]
    struct Observed(f64);

    #[derive(StateTreeInput)]
    struct Observe;
    impl inslag_state_tree::StateTreeBinding for Observe {
        type Value = f64;
        type Event = Observation;
        fn event(&self, entity: Entity, value: &f64) -> Observation {
            Observation {
                entity,
                value: *value,
            }
        }
    }

    #[derive(EntityEvent, inslag_state_tree::StateTreeEvent)]
    #[state_tree(name = "world.changed", data = observation_data)]
    struct Observation {
        entity: Entity,
        value: f64,
    }

    fn observation_data(event: &Observation) -> LuauValue {
        LuauValue::Number(event.value)
    }

    #[test]
    fn hover_actions_world_observations_and_view_bindings_share_one_update() {
        use bevy::picking::{backend::HitData, hover::HoverMap, pointer::PointerId};
        let source = format!(
            r#"{HEADER}<node on-hover="track"><text id="position" bind:text="x"/><text id="observed" bind:text="observed"/></node></view>"#
        );
        let mut app = test_support::app(&[("input.kui", &source)]);
        app.init_resource::<Observed>()
            .init_resource::<HoverMap>()
            .add_plugins((StateTreePlugin::new(Update), KuiStateTreePlugin))
            .add_state_tree_global_binding::<Observe, _, _>(
                "world.observe",
                |value: Res<Observed>| value.0,
            )
            .add_observer(|event: On<KuiAction>, mut observed: ResMut<Observed>| {
                if let Some(position) = event.data.position {
                    observed.0 = position.x as f64;
                }
            });
        let chart = r#"<scxml xmlns="http://www.w3.org/2005/07/scxml" xmlns:bst="urn:inslag-state-tree" datamodel="luau" initial="active">
          <datamodel><data id="x" expr="0"/><data id="observed" expr="0"/></datamodel>
          <state id="active">
            <bst:activity id="view" name="ui.render"><bst:arg name="src" expr="'input.kui'"/><bst:arg name="space" expr="'world'"/></bst:activity>
            <bst:activity id="observation" name="world.observe"/>
            <transition event="ui.action" cond="_event.data.action == 'track'"><assign location="x" expr="_event.data.position.x"/></transition>
            <transition event="world.changed"><assign location="observed" expr="_event.data"/></transition>
          </state></scxml>"#;
        let definition = scxml::compile(&scxml::parse("input.scxml", chart).unwrap()).unwrap();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<StateTreeDefinition>>()
            .add(definition);
        let owner = app
            .world_mut()
            .spawn((
                Transform::default(),
                Visibility::default(),
                StateTreeInstance::new(handle),
            ))
            .id();
        wait_for(&mut app, |app| {
            app.world_mut().query::<&Text2d>().iter(app.world()).count() == 2
        });
        // Let the activity's initial observation be delivered before the interaction.
        app.update();
        let text = app
            .world_mut()
            .query::<(Entity, &Name)>()
            .iter(app.world())
            .find(|(_, name)| name.as_str() == "position")
            .unwrap()
            .0;
        app.world_mut().resource_mut::<HoverMap>().0.insert(
            PointerId::Mouse,
            [(
                text,
                HitData::new(Entity::PLACEHOLDER, 1., Some(Vec3::new(42.5, 7., 0.)), None),
            )]
            .into_iter()
            .collect(),
        );
        app.update();
        let instance = app.world().get::<StateTreeInstance>(owner).unwrap();
        assert_eq!(instance.data("x"), Some(&LuauValue::Number(42.5)));
        assert_eq!(instance.data("observed"), Some(&LuauValue::Number(42.5)));
        assert!(
            app.world_mut()
                .query::<&Text2d>()
                .iter(app.world())
                .all(|text| text.0 == "42.5")
        );
    }

    #[test]
    fn authored_example_owns_dragging_and_cancel_restores_the_controller_position() {
        let mut app = test_support::app(&[(
            "interactions.kui",
            include_str!("../assets/interactions.kui"),
        )]);
        app.add_plugins((StateTreePlugin::new(Update), KuiStateTreePlugin));
        let definition = scxml::compile(
            &scxml::parse(
                "interactions.scxml",
                include_str!("../assets/interactions.scxml"),
            )
            .unwrap(),
        )
        .unwrap();
        let definition = app
            .world_mut()
            .resource_mut::<Assets<StateTreeDefinition>>()
            .add(definition);
        let owner = app
            .world_mut()
            .spawn((
                Transform::default(),
                Visibility::default(),
                StateTreeInstance::new(definition),
            ))
            .id();
        wait_for(&mut app, |app| {
            app.world_mut()
                .query::<&KuiView>()
                .iter(app.world())
                .any(|view| view.status() == &KuiViewStatus::Ready)
        });
        let mount = app
            .world_mut()
            .query_filtered::<Entity, With<KuiView>>()
            .single(app.world())
            .unwrap();
        let label = app
            .world_mut()
            .query::<(Entity, &Name)>()
            .iter(app.world())
            .find(|(_, name)| name.as_str() == "label")
            .unwrap()
            .0;
        let send = |app: &mut App, action: &str, position: Vec2| {
            app.world_mut().trigger(KuiAction {
                entity: owner,
                view: mount,
                action: action.into(),
                value: None,
                data: crate::KuiActionData {
                    position: Some(position),
                    start: Some(Vec2::new(10., 20.)),
                },
            });
            app.world_mut().flush();
            app.update();
            assert!(matches!(
                app.world()
                    .get::<StateTreeInstance>(owner)
                    .unwrap()
                    .status(),
                InstanceStatus::Running
            ));
        };
        send(&mut app, "lift", Vec2::new(10., 20.));
        send(&mut app, "move", Vec2::new(50., 70.));
        assert_eq!(
            app.world().get::<Transform>(label).unwrap().translation,
            Vec3::new(40., 50., 1.)
        );
        send(&mut app, "drop", Vec2::new(70., 100.));
        assert_eq!(
            app.world().get::<Transform>(label).unwrap().translation,
            Vec3::new(60., 80., 1.)
        );
        send(&mut app, "lift", Vec2::new(10., 20.));
        send(&mut app, "move", Vec2::new(30., 40.));
        assert_eq!(
            app.world().get::<Transform>(label).unwrap().translation,
            Vec3::new(80., 100., 1.)
        );
        send(&mut app, "cancel", Vec2::new(30., 40.));
        assert_eq!(
            app.world().get::<Transform>(label).unwrap().translation,
            Vec3::new(60., 80., 1.)
        );
    }
}
