use std::collections::BTreeMap;

use bevy::{
    asset::{AssetEvent, AssetLoadFailedEvent, UntypedAssetId},
    picking::hover::Hovered,
    prelude::*,
    ui::InteractionDisabled,
    ui_widgets::Button,
};
use bevy_flair::prelude::{AttributeList, ClassList, StyleSheet, Styled, TypeName};
use bevy_flair::style::StyleSystems;

use crate::{
    KuiAsset, KuiAssetLoader, KuiBinding, KuiNode, KuiNodeKind, KuiTextFormat, KuiTextPart,
};

mod collections;
pub(crate) use collections::KuiComponentInstance;
use collections::RuntimeNode;
#[cfg(test)]
pub(crate) use collections::{KuiItemKey, KuiRepeatItem};

pub(crate) struct KuiAssetPlugin;

impl Plugin for KuiAssetPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<KuiAsset>()
            .init_asset::<StyleSheet>()
            .register_asset_loader(KuiAssetLoader);
    }
}

pub(crate) struct KuiPlugin;

/// Ordering points for adapters and application policy, all in `Update`.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum KuiSystems {
    /// Samples interactions before state-tree observations and presentation.
    Input,
    Prepare,
    Mount,
    Bind,
    Outcomes,
}

impl Plugin for KuiPlugin {
    fn finish(&self, app: &mut App) {
        crate::motion::install(app);
    }

    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<KuiAssetPlugin>() {
            app.add_plugins(KuiAssetPlugin);
        }
        // Register before Flair's loader snapshots its property registry in
        // finish(). finish() below covers headless/test apps adding Flair later.
        crate::motion::install(app);
        crate::interaction::install(app);
        app.configure_sets(
            Update,
            (
                KuiSystems::Prepare,
                KuiSystems::Mount,
                KuiSystems::Bind,
                KuiSystems::Outcomes,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (fail_assets, reload_assets, spawn_views)
                .chain()
                .in_set(KuiSystems::Mount),
        )
        .add_systems(
            Update,
            (collections::reconcile, sync_views)
                .chain()
                .in_set(KuiSystems::Bind),
        )
        .add_systems(PostUpdate, restore_display.before(StyleSystems::Prepare))
        .add_systems(
            PostUpdate,
            apply_visibility
                .after(restore_display)
                .after(StyleSystems::ApplyComputedProperties)
                .before(bevy::ui::UiSystems::Content),
        )
        .add_systems(
            PostUpdate,
            apply_meters
                .after(StyleSystems::ApplyComputedProperties)
                .before(bevy::ui::UiSystems::Content),
        );
    }
}

/// Portable binding values shared with the Luau engine and state trees.
pub use inslag_luau::LuauValue as KuiValue;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum KuiViewStatus {
    Loading,
    Ready,
    Failed(String),
}

/// One independent mount. Spawn this on a dedicated entity; despawning that
/// entity or its owner also removes its visual roots.
#[derive(Component)]
#[require(bevy::ui::experimental::GhostNode)]
#[component(on_add = attach_mount)]
pub(crate) struct KuiView {
    pub world_space: bool,
    pub owner: Entity,
    pub asset: Handle<KuiAsset>,
    pub data: BTreeMap<String, KuiValue>,
    status: KuiViewStatus,
    roots: Vec<Entity>,
    tree: Vec<RuntimeNode>,
    mounted_asset: Option<AssetId<KuiAsset>>,
    bindings_pending: bool,
    expressions: crate::expression::Evaluator,
}

fn attach_mount(
    mut world: bevy::ecs::world::DeferredWorld,
    context: bevy::ecs::lifecycle::HookContext,
) {
    let owner = world
        .get::<KuiView>(context.entity)
        .expect("added mount")
        .owner;
    world
        .commands()
        .entity(owner)
        .insert_if_new(bevy::ui::experimental::GhostNode);
    world
        .commands()
        .entity(context.entity)
        .insert(ChildOf(owner));
}

impl KuiView {
    pub fn new(owner: Entity, asset: Handle<KuiAsset>) -> Self {
        Self {
            world_space: false,
            owner,
            asset,
            data: BTreeMap::new(),
            status: KuiViewStatus::Loading,
            roots: Vec::new(),
            tree: Vec::new(),
            mounted_asset: None,
            bindings_pending: false,
            expressions: default(),
        }
    }

    pub fn status(&self) -> &KuiViewStatus {
        &self.status
    }

    /// Mount the same presentation vocabulary using world transforms and sprites.
    #[cfg(test)]
    pub fn in_world(mut self) -> Self {
        self.world_space = true;
        self
    }

    #[cfg(test)]
    pub fn roots(&self) -> &[Entity] {
        &self.roots
    }
}

/// Internal presentation boundary for scoped motion.
#[derive(Component)]
pub(crate) struct KuiViewRoot {
    #[cfg(test)]
    pub controller: Entity,
}

/// An authored interaction action, targeted at the mount's owner.
#[derive(Clone, Debug, EntityEvent)]
pub(crate) struct KuiAction {
    pub entity: Entity,
    pub view: Entity,
    pub action: String,
    pub value: Option<String>,
    pub data: crate::interaction::KuiActionData,
}

#[derive(Component)]
pub(crate) struct ActionBindings {
    pub view: Entity,
    pub events: BTreeMap<String, String>,
    pub value: Option<String>,
    pub value_valid: bool,
}

#[derive(Component)]
pub(crate) struct TabIndex(pub i32);

struct RuntimeBinding {
    entity: Entity,
    path: String,
    operation: BindingOperation,
    previous: Option<KuiValue>,
    values: Vec<Option<KuiValue>>,
}

enum BindingOperation {
    Property(String),
    Meter {
        current: String,
        maximum: String,
    },
    Value,
    Text(KuiTextFormat),
    TextTemplate {
        parts: Vec<KuiTextPart>,
        paths: Vec<String>,
    },
    Visible,
    Disabled,
    Attribute(String),
}

/// A reversible layout override. Flair always sees the unmodified display mode,
/// so stylesheet changes (including `display: none`) remain authoritative when
/// the binding releases its override.
#[derive(Component)]
pub(crate) struct BoundVisibility {
    pub(crate) visible: bool,
    styled_display: Option<Display>,
    previous_display: Display,
}

#[derive(Component)]
struct BoundMeter {
    fraction: f32,
    displayed: Option<f32>,
}

fn apply_meters(mut fills: Query<(&mut Node, &mut BoundMeter)>, time: Option<Res<Time>>) {
    for (mut node, mut meter) in &mut fills {
        let target = meter.fraction;
        let displayed = match (meter.displayed, time.as_ref()) {
            (Some(previous), Some(time)) => {
                let next =
                    previous + (target - previous) * (1.0 - (-18.0 * time.delta_secs()).exp());
                if (next - target).abs() < 0.0001 {
                    target
                } else {
                    next
                }
            }
            _ => target,
        };
        let previous = meter.displayed;
        meter.displayed = Some(displayed);
        node.bypass_change_detection().width = Val::Percent(displayed * 100.0);
        if previous != Some(displayed) {
            node.set_changed();
        }
    }
}

fn restore_display(mut nodes: Query<(&mut Node, &mut BoundVisibility)>) {
    for (mut node, mut visibility) in &mut nodes {
        visibility.previous_display = node.display;
        if let Some(display) = visibility.styled_display.take() {
            // This temporary restoration is invisible to layout and must not
            // mark an otherwise unchanged hidden node as changed every frame.
            node.bypass_change_detection().display = display;
        }
    }
}

fn apply_visibility(mut nodes: Query<(&mut Node, &mut BoundVisibility)>) {
    for (mut node, mut visibility) in &mut nodes {
        if !visibility.visible {
            visibility.styled_display = Some(node.display);
            node.bypass_change_detection().display = Display::None;
        }
        if node.display != visibility.previous_display {
            node.set_changed();
        }
    }
}

fn fail_assets(
    mut kui: MessageReader<AssetLoadFailedEvent<KuiAsset>>,
    mut styles: MessageReader<AssetLoadFailedEvent<StyleSheet>>,
    mut images: MessageReader<AssetLoadFailedEvent<Image>>,
    assets: Res<Assets<KuiAsset>>,
    asset_server: Res<AssetServer>,
    mut views: Query<&mut KuiView>,
    mut commands: Commands,
) {
    let failures: Vec<_> = kui
        .read()
        .map(AssetLoadFailedEvent::untyped)
        .chain(styles.read().map(AssetLoadFailedEvent::untyped))
        .chain(images.read().map(AssetLoadFailedEvent::untyped))
        .collect();
    for mut view in &mut views {
        if matches!(view.status, KuiViewStatus::Failed(_))
            || (failures.is_empty() && view.status == KuiViewStatus::Ready && !view.is_changed())
        {
            continue;
        }
        let failure = failures
            .iter()
            .find(|failure| failure_matches(&view, &assets, failure.id))
            .map(|failure| {
                format!(
                    "could not load KUI dependency `{}`: {}",
                    failure.path, failure.error
                )
            })
            .or_else(|| {
                if view.status == KuiViewStatus::Ready && !view.is_changed() {
                    return None;
                }
                match asset_server.get_recursive_dependency_load_state(view.asset.id()) {
                    Some(bevy::asset::RecursiveDependencyLoadState::Failed(error)) => {
                        Some(error.to_string())
                    }
                    _ => match asset_server.get_load_state(view.asset.id()) {
                        Some(bevy::asset::LoadState::Failed(error)) => Some(error.to_string()),
                        _ => None,
                    },
                }
            });
        if let Some(message) = failure {
            clear_view(&mut view, &mut commands);
            view.status = KuiViewStatus::Failed(message);
        }
    }
}

fn failure_matches(view: &KuiView, assets: &Assets<KuiAsset>, id: UntypedAssetId) -> bool {
    view.asset.id().untyped() == id
        || assets.get(&view.asset).is_some_and(|asset| {
            asset
                .style_sheet
                .iter()
                .any(|handle| handle.id().untyped() == id)
                || asset
                    .image_handles
                    .iter()
                    .any(|handle| handle.id().untyped() == id)
                || asset
                    .component_style_handles
                    .iter()
                    .any(|handle| handle.id().untyped() == id)
        })
}

fn reload_assets(
    mut events: MessageReader<AssetEvent<KuiAsset>>,
    mut views: Query<(Entity, &mut KuiView)>,
    mut commands: Commands,
) {
    let changed: Vec<_> = events
        .read()
        .filter_map(|event| match event {
            AssetEvent::Modified { id } | AssetEvent::Removed { id } => Some(*id),
            _ => None,
        })
        .collect();
    for (entity, mut view) in &mut views {
        if changed.contains(&view.asset.id())
            || view.mounted_asset.is_some_and(|id| id != view.asset.id())
        {
            clear_view(&mut view, &mut commands);
            view.status = KuiViewStatus::Loading;
            crate::state_tree::invalidate_view(&mut commands, entity);
        }
    }
}

fn clear_view(view: &mut KuiView, commands: &mut Commands) {
    for root in view.roots.drain(..) {
        commands.entity(root).despawn();
    }
    view.tree.clear();
    view.expressions = default();
    view.mounted_asset = None;
}

fn spawn_views(
    mut commands: Commands,
    assets: Res<Assets<KuiAsset>>,
    mut views: Query<(Entity, &mut KuiView)>,
    cameras: Query<&UiTargetCamera>,
) {
    for (view_entity, mut view) in &mut views {
        if !matches!(view.status, KuiViewStatus::Loading) {
            continue;
        }
        let Some(asset) = assets.get(&view.asset) else {
            continue;
        };
        if let Err(message) = validate_context(&asset.definition.children, view.world_space) {
            view.status = KuiViewStatus::Failed(message);
            continue;
        }
        if view.world_space {
            commands
                .entity(view_entity)
                .insert_if_new((Transform::default(), Visibility::default()));
        }
        for node in &asset.definition.children {
            let tree = collections::spawn(
                &mut commands,
                Some(view_entity),
                view_entity,
                node,
                asset,
                view.world_space,
                false,
            );
            let root = tree.entity().expect("validated visual root");
            commands.entity(root).insert(KuiViewRoot {
                #[cfg(test)]
                controller: view.owner,
            });
            if !view.world_space
                && let Ok(camera) = cameras.get(view.owner)
            {
                commands.entity(root).insert(camera.clone());
            }
            if let Some(style) = &asset.style_sheet
                && tree.root_inherits_stylesheet()
            {
                commands.entity(root).insert(Styled::new(style.clone()));
            }
            view.roots.push(root);
            view.tree.push(tree);
        }
        view.mounted_asset = Some(view.asset.id());
        view.status = KuiViewStatus::Ready;
    }
}

fn spawn_node(
    commands: &mut Commands,
    parent: Option<Entity>,
    view: Entity,
    node: &KuiNode,
    asset: &KuiAsset,
    world_space: bool,
) -> (Entity, Vec<RuntimeBinding>) {
    let mut entity = if world_space {
        match &node.kind {
            KuiNodeKind::Image { src } => {
                let index = asset
                    .image_paths
                    .iter()
                    .position(|path| path == src)
                    .expect("compiled image");
                let mut sprite = Sprite::from_image(asset.image_handles[index].clone());
                if let Some(crop) = node.attributes.get("crop") {
                    let v: Vec<f32> = crop
                        .split_whitespace()
                        .map(|n| n.parse().expect("validated crop"))
                        .collect();
                    sprite.rect = Some(Rect::new(v[0], v[1], v[2], v[3]));
                }
                commands.spawn((sprite, Pickable::IGNORE))
            }
            KuiNodeKind::Text { value } => commands.spawn((Text2d::new(value), Pickable::IGNORE)),
            _ => commands.spawn((
                Transform::default(),
                Visibility::default(),
                Pickable::IGNORE,
            )),
        }
    } else {
        match &node.kind {
            KuiNodeKind::Node { .. } => commands.spawn((
                Node::default(),
                Styled::default(),
                TypeName("node"),
                Pickable::IGNORE,
            )),
            KuiNodeKind::Meter { .. } => commands.spawn((
                Node::default(),
                Styled::default(),
                TypeName("meter"),
                ClassList::new("meter"),
                Pickable::IGNORE,
            )),
            KuiNodeKind::Button { .. } => commands.spawn((
                Node::default(),
                Button,
                Styled::default(),
                TypeName("button"),
            )),
            KuiNodeKind::Text { value } => commands.spawn((
                Text(value.clone()),
                Styled::default(),
                TypeName("text"),
                Pickable::IGNORE,
            )),
            KuiNodeKind::Image { src } => {
                let index = asset
                    .image_paths
                    .iter()
                    .position(|path| path == src)
                    .expect("KUI image paths and handles are compiled together");
                commands.spawn((
                    Node::default(),
                    ImageNode::new(asset.image_handles[index].clone()),
                    Styled::default(),
                    TypeName("image"),
                    Pickable::IGNORE,
                ))
            }
            KuiNodeKind::Repeat { .. }
            | KuiNodeKind::Scope { .. }
            | KuiNodeKind::Caller { .. }
            | KuiNodeKind::Slot { .. }
            | KuiNodeKind::Use { .. } => unreachable!("structural nodes are reconciled separately"),
        }
    };
    if world_space {
        let kind = match node.kind {
            KuiNodeKind::Image { .. } => "image",
            KuiNodeKind::Text { .. } => "text",
            KuiNodeKind::Button { .. } => "button",
            _ => "node",
        };
        entity.insert((
            Styled::default(),
            TypeName(kind),
            UiTransform::default(),
            crate::motion::WorldPresentation::default(),
        ));
    }
    if let Some(parent) = parent {
        entity.insert(ChildOf(parent));
    }
    apply_element_attributes(&mut entity, view, node, world_space);
    if matches!(node.kind, KuiNodeKind::Button { .. }) && !node.attributes.contains_key("tab-index")
    {
        entity.insert(TabIndex(0));
    }
    if let Some(id) = &node.id {
        entity.insert(Name::new(id.clone()));
    }
    if let Some(class) = &node.class {
        entity.insert(ClassList::new(
            &if matches!(node.kind, KuiNodeKind::Meter { .. }) {
                format!("meter {class}")
            } else {
                class.clone()
            },
        ));
    }
    initialize_scope_bindings(&mut entity, &node.bindings, world_space);
    let entity_id = entity.id();
    let mut bindings = runtime_bindings(entity_id, &node.bindings);
    if let KuiNodeKind::Meter { current, maximum } = &node.kind {
        let fill = commands
            .spawn((
                Node::default(),
                Styled::default(),
                TypeName("node"),
                ClassList::new("meter-fill"),
                Pickable::IGNORE,
                ChildOf(entity_id),
                BoundMeter {
                    fraction: 0.0,
                    displayed: None,
                },
            ))
            .id();
        bindings.push(RuntimeBinding {
            entity: fill,
            path: current.clone(),
            operation: BindingOperation::Meter {
                current: current.clone(),
                maximum: maximum.clone(),
            },
            previous: None,
            values: vec![],
        });
    }
    (entity_id, bindings)
}

fn validate_context(nodes: &[KuiNode], world_space: bool) -> Result<(), String> {
    for node in nodes {
        if world_space && matches!(node.kind, KuiNodeKind::Meter { .. }) {
            return Err("<meter> currently requires a layout presentation context".into());
        }
        if !world_space
            && (node.attributes.keys().any(|key| {
                matches!(
                    key.as_str(),
                    "x" | "y" | "z" | "scale-x" | "scale-y" | "crop"
                )
            }) || node
                .bindings
                .iter()
                .any(|b| matches!(b, KuiBinding::Property { name, .. } if !matches!(name.as_str(), "x" | "y" | "right" | "bottom"))))
        {
            return Err(
                "transform and crop attributes currently require a world presentation context"
                    .into(),
            );
        }
        match &node.kind {
            KuiNodeKind::Node { children }
            | KuiNodeKind::Button { children, .. }
            | KuiNodeKind::Slot { children, .. }
            | KuiNodeKind::Caller { children, .. } => validate_context(children, world_space)?,
            KuiNodeKind::Scope { template, .. } | KuiNodeKind::Repeat { template, .. } => {
                validate_context(std::slice::from_ref(template), world_space)?
            }
            _ => {}
        }
    }
    Ok(())
}

fn apply_element_attributes(
    entity: &mut EntityCommands,
    view: Entity,
    node: &KuiNode,
    world_space: bool,
) {
    if let Some(index) = node.attributes.get("tab-index") {
        entity.insert(TabIndex(index.parse().expect("validated tab-index")));
    }
    if world_space
        && node
            .attributes
            .keys()
            .any(|key| matches!(key.as_str(), "x" | "y" | "z" | "scale-x" | "scale-y"))
    {
        let number = |name: &str, fallback| {
            node.attributes
                .get(name)
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(fallback)
        };
        entity.insert(
            Transform::from_xyz(number("x", 0.0), number("y", 0.0), number("z", 0.0)).with_scale(
                Vec3::new(number("scale-x", 1.0), number("scale-y", 1.0), 1.0),
            ),
        );
    }
    let events: BTreeMap<_, _> = node
        .attributes
        .iter()
        .filter(|(key, _)| crate::interaction::is_event_attribute(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    if !events.is_empty()
        || node.attributes.contains_key("cursor")
        || matches!(node.kind, KuiNodeKind::Button { .. })
    {
        entity.insert(Hovered::default());
    }
    if !events.is_empty() {
        let value = node.attributes.get("value").cloned();
        let value_valid = !node
            .bindings
            .iter()
            .any(|b| matches!(b, KuiBinding::Value(_)));
        let overrides_value = value.is_some() || !value_valid;
        let added_events = events.clone();
        let added_value = value.clone();
        entity.insert(Pickable::default());
        // A component use shares its visual root. Adding a handler must preserve
        // the root's other event mappings and its payload unless explicitly replaced.
        entity
            .entry::<ActionBindings>()
            .and_modify(move |mut bindings| {
                bindings.events.extend(added_events);
                if overrides_value {
                    bindings.value = added_value;
                    bindings.value_valid = value_valid;
                }
            })
            .or_insert(ActionBindings {
                view,
                events,
                value,
                value_valid,
            });
    }
    if let Some(cursor) = node.attributes.get("cursor") {
        entity.insert(crate::interaction::HoverCursor(match cursor.as_str() {
            "grab" => bevy::window::SystemCursorIcon::Grab,
            "pointer" => bevy::window::SystemCursorIcon::Pointer,
            _ => bevy::window::SystemCursorIcon::Default,
        }));
    }
}

fn initialize_scope_bindings(
    entity: &mut EntityCommands,
    authored: &[KuiBinding],
    world_space: bool,
) {
    for binding in authored {
        match binding {
            KuiBinding::Attribute { .. } => {
                entity.insert(AttributeList::new());
            }
            KuiBinding::Visible(_) if !world_space => {
                entity.insert(BoundVisibility {
                    visible: true,
                    styled_display: None,
                    previous_display: Display::default(),
                });
            }
            KuiBinding::Disabled(_) => {
                entity.insert(InteractionDisabled);
            }
            _ => {}
        }
    }
}

fn runtime_bindings(entity_id: Entity, authored: &[KuiBinding]) -> Vec<RuntimeBinding> {
    authored
        .iter()
        .map(|binding| {
            let (path, operation) = match binding {
                KuiBinding::Property { name, path } => {
                    (path.as_str(), BindingOperation::Property(name.clone()))
                }
                KuiBinding::Value(path) => (path.as_str(), BindingOperation::Value),
                KuiBinding::Text(path) => {
                    (path.as_str(), BindingOperation::Text(KuiTextFormat::Plain))
                }
                KuiBinding::FormattedText { path, format } => {
                    (path.as_str(), BindingOperation::Text(format.clone()))
                }
                KuiBinding::TextTemplate(parts) => (
                    "template",
                    BindingOperation::TextTemplate {
                        parts: parts.clone(),
                        paths: parts
                            .iter()
                            .filter_map(|part| match part {
                                KuiTextPart::Binding { path, .. } => Some(path.clone()),
                                _ => None,
                            })
                            .collect(),
                    },
                ),
                KuiBinding::Visible(path) => (path.as_str(), BindingOperation::Visible),
                KuiBinding::Disabled(path) => (path.as_str(), BindingOperation::Disabled),
                KuiBinding::Attribute { name, expression } => (
                    expression.as_str(),
                    BindingOperation::Attribute(name.clone()),
                ),
            };
            RuntimeBinding {
                entity: entity_id,
                path: path.to_owned(),
                operation,
                previous: None,
                values: vec![],
            }
        })
        .collect::<Vec<_>>()
}

fn sync_views(
    mut views: Query<&mut KuiView>,
    mut texts: Query<&mut Text>,
    mut visibilities: Query<&mut BoundVisibility>,
    mut world_visibility: Query<&mut Visibility>,
    mut transforms: Query<&mut Transform>,
    mut world_texts: Query<&mut Text2d>,
    mut attributes: Query<&mut AttributeList>,
    mut buttons: Query<&mut ActionBindings>,
    mut meters: Query<&mut BoundMeter>,
    mut commands: Commands,
    mut nodes: Query<&mut Node>,
) {
    for mut view in &mut views {
        if !view.bindings_pending {
            continue;
        }
        // Binding caches are private bookkeeping, not new application input.
        let view = view.bypass_change_detection();
        view.bindings_pending = false;
        let mut bindings = Vec::new();
        for tree in &mut view.tree {
            tree.bindings(&mut bindings);
        }
        for binding in bindings {
            if matches!(binding.operation, BindingOperation::Meter { .. }) {
                let fraction = match binding.values.as_slice() {
                    [Some(current), Some(maximum)] => meter_fraction(current, maximum),
                    _ => None,
                };
                if let Some(fraction) = fraction {
                    if let Ok(mut meter) = meters.get_mut(binding.entity)
                        && meter.fraction != fraction
                    {
                        meter.fraction = fraction;
                    }
                } else {
                    warn_once!(
                        "KUI meter `{}` requires finite numeric current and maximum",
                        binding.path
                    );
                }
                continue;
            }
            if let BindingOperation::TextTemplate { parts, paths } = &binding.operation {
                let values = binding.values.iter().cloned().collect::<Option<Vec<_>>>();
                if let (Some(values), Some(KuiValue::Sequence(previous))) =
                    (&values, &binding.previous)
                    && values == previous
                {
                    continue;
                }
                let rendered = values.as_ref().and_then(|values| {
                    let mut values = values.iter();
                    let mut result = String::new();
                    for part in parts {
                        match part {
                            KuiTextPart::Literal(literal) => result.push_str(literal),
                            KuiTextPart::Binding { format, .. } => {
                                result.push_str(&format.render(values.next()?)?)
                            }
                        }
                    }
                    Some(result)
                });
                if let (Some(values), Some(rendered)) = (values, rendered) {
                    let value = KuiValue::Sequence(values);
                    if let Ok(mut text) = texts.get_mut(binding.entity) {
                        if text.0 != rendered {
                            text.0 = rendered;
                        }
                        binding.previous = Some(value);
                    } else if let Ok(mut text) = world_texts.get_mut(binding.entity) {
                        if text.0 != rendered {
                            text.0 = rendered;
                        }
                        binding.previous = Some(value);
                    }
                } else {
                    warn_once!("KUI text template has missing or incompatible values: {paths:?}");
                }
                continue;
            }
            let Some(value) = binding.values.first().and_then(Option::as_ref) else {
                if matches!(binding.operation, BindingOperation::Value) {
                    if let Ok(mut button) = buttons.get_mut(binding.entity) {
                        button.value_valid = false;
                    }
                    binding.previous = None;
                }
                if matches!(binding.operation, BindingOperation::Disabled) {
                    commands.entity(binding.entity).insert(InteractionDisabled);
                    binding.previous = None;
                }
                warn_once!(
                    "KUI binding `{}` does not exist in its owner's data",
                    binding.path
                );
                continue;
            };
            if binding.previous.as_ref() == Some(value) {
                continue;
            }
            let applied = match (&binding.operation, value) {
                (BindingOperation::Property(name), value) => {
                    if matches!(name.as_str(), "position" | "scale") {
                        let coordinate = |key: &str| -> Option<f32> {
                            let KuiValue::Record(record) = value else {
                                return None;
                            };
                            let number = match record.get(key)? {
                                KuiValue::Number(n) => *n as f32,
                                KuiValue::Integer(n) => *n as f32,
                                _ => return None,
                            };
                            number.is_finite().then_some(number)
                        };
                        let compound = coordinate("x").zip(coordinate("y")).and_then(|(x, y)| {
                            if name == "position" {
                                Some(Vec3::new(x, y, coordinate("z")?))
                            } else {
                                Some(Vec3::new(x, y, 1.0))
                            }
                        });
                        if let (Some(coordinates), Ok(mut transform)) =
                            (compound, transforms.get_mut(binding.entity))
                        {
                            if name == "position" {
                                transform.translation = coordinates;
                            } else {
                                transform.scale = coordinates;
                            }
                            binding.previous = Some(value.clone());
                        } else {
                            warn_once!(
                                "KUI compound binding `{}` requires finite coordinates",
                                binding.path
                            );
                        }
                        continue;
                    }
                    let value = match value {
                        KuiValue::Number(n) => Some(*n as f32),
                        KuiValue::Integer(n) => Some(*n as f32),
                        _ => None,
                    };
                    if matches!(name.as_str(), "x" | "y" | "right" | "bottom")
                        && let (Some(value), Ok(mut node)) = (
                            value.filter(|v| v.is_finite()),
                            nodes.get_mut(binding.entity),
                        )
                    {
                        let target = match name.as_str() {
                            "x" => &mut node.left,
                            "y" => &mut node.top,
                            "right" => &mut node.right,
                            "bottom" => &mut node.bottom,
                            _ => unreachable!(),
                        };
                        if *target != Val::Px(value) {
                            *target = Val::Px(value);
                        }
                        binding.previous = binding.values[0].clone();
                        continue;
                    }
                    if matches!(name.as_str(), "right" | "bottom") {
                        warn_once!("KUI edge bindings require a screen layout node");
                        continue;
                    }
                    if let (Some(value), Ok(mut transform)) = (
                        value.filter(|v| v.is_finite()),
                        transforms.get_mut(binding.entity),
                    ) {
                        match name.as_str() {
                            "x" => transform.translation.x = value,
                            "y" => transform.translation.y = value,
                            "z" => transform.translation.z = value,
                            "scale-x" => transform.scale.x = value,
                            "scale-y" => transform.scale.y = value,
                            "rotation" => transform.rotation = Quat::from_rotation_z(value),
                            _ => unreachable!("validated property"),
                        }
                        true
                    } else {
                        false
                    }
                }
                (BindingOperation::Value, value) => {
                    if let Ok(mut button) = buttons.get_mut(binding.entity) {
                        let payload = match value {
                            KuiValue::Nil => Some(None),
                            KuiValue::String(value) => Some(Some(value.clone())),
                            KuiValue::Integer(value) => Some(Some(value.to_string())),
                            KuiValue::Boolean(value) => Some(Some(value.to_string())),
                            KuiValue::Number(value) if value.is_finite() => {
                                Some(Some(value.to_string()))
                            }
                            _ => None,
                        };
                        button.value_valid = payload.is_some();
                        if let Some(payload) = payload {
                            button.value = payload;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                (BindingOperation::Text(format), value) => {
                    if let (Ok(mut text), Some(value)) =
                        (texts.get_mut(binding.entity), format.render(value))
                    {
                        if text.0 != value {
                            text.0 = value;
                        }
                        true
                    } else if let (Ok(mut text), Some(value)) =
                        (world_texts.get_mut(binding.entity), format.render(value))
                    {
                        text.0 = value;
                        true
                    } else {
                        false
                    }
                }
                (BindingOperation::Visible, KuiValue::Boolean(value)) => {
                    if let Ok(mut visibility) = visibilities.get_mut(binding.entity) {
                        if visibility.visible != *value {
                            visibility.visible = *value;
                        }
                        true
                    } else if let Ok(mut visibility) = world_visibility.get_mut(binding.entity) {
                        visibility.set_if_neq(if *value {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        });
                        true
                    } else {
                        false
                    }
                }
                (BindingOperation::Disabled, KuiValue::Boolean(value)) => {
                    if *value {
                        commands.entity(binding.entity).insert(InteractionDisabled);
                    } else {
                        commands
                            .entity(binding.entity)
                            .remove::<InteractionDisabled>();
                    }
                    true
                }
                (BindingOperation::Attribute(name), value) => {
                    if let Ok(mut attributes) = attributes.get_mut(binding.entity) {
                        match value {
                            KuiValue::Nil => {
                                attributes.remove_attribute(name);
                                true
                            }
                            KuiValue::Boolean(value) => {
                                attributes.set_attribute(name.clone(), value.to_string());
                                true
                            }
                            KuiValue::Integer(value) => {
                                attributes.set_attribute(name.clone(), value.to_string());
                                true
                            }
                            KuiValue::Number(value) if value.is_finite() => {
                                attributes.set_attribute(name.clone(), value.to_string());
                                true
                            }
                            KuiValue::String(value) => {
                                attributes.set_attribute(name.clone(), value.clone());
                                true
                            }
                            _ => false,
                        }
                    } else {
                        false
                    }
                }
                (BindingOperation::Disabled, _) => {
                    commands.entity(binding.entity).insert(InteractionDisabled);
                    binding.previous = None;
                    false
                }
                _ => false,
            };
            if applied {
                binding.previous = Some(value.clone());
            } else {
                if matches!(binding.operation, BindingOperation::Value) {
                    binding.previous = None;
                }
                warn_once!(
                    "KUI binding `{}` has an incompatible value or target",
                    binding.path
                );
            }
        }
    }
}

fn meter_fraction(current: &KuiValue, maximum: &KuiValue) -> Option<f32> {
    fn number(value: &KuiValue) -> Option<f64> {
        match value {
            KuiValue::Integer(v) => Some(*v as f64),
            KuiValue::Number(v) if v.is_finite() => Some(*v),
            _ => None,
        }
    }
    let current = number(current)?;
    let maximum = number(maximum)?;
    Some(if maximum <= 0.0 {
        0.0
    } else {
        (current / maximum).clamp(0.0, 1.0) as f32
    })
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use bevy::asset::io::{
        AssetSourceBuilder, AssetSourceId,
        memory::{Dir, MemoryAssetReader},
    };

    pub fn app(assets: &[(&str, &str)]) -> App {
        let directory = Dir::default();
        for (path, source) in assets {
            directory.insert_asset_text(std::path::Path::new(path), source);
        }
        let mut app = App::new();
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(move || {
                Box::new(MemoryAssetReader {
                    root: directory.clone(),
                })
            }),
        );
        app.add_plugins((
            bevy::app::TaskPoolPlugin::default(),
            bevy::time::TimePlugin,
            AssetPlugin {
                watch_for_changes_override: Some(false),
                use_asset_processor_override: Some(false),
                ..default()
            },
        ));
        app.init_asset::<Image>().add_plugins(KuiPlugin);
        app
    }
}
