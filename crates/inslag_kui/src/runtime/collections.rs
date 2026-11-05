use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::*;

use super::{
    KuiAsset, KuiNode, KuiNodeKind, KuiValue, KuiView, KuiViewStatus, RuntimeBinding, spawn_node,
};

/// Stable collection identity. String and integer keys are distinct.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum KuiItemKey {
    String(String),
    Integer(i64),
}

/// Attached to each repeated row's visual root. Nested row scopes can be found
/// by following `ChildOf`; keys are local to their collection instance.
#[derive(Component, Clone, Debug)]
#[cfg(test)]
pub(crate) struct KuiRepeatItem {
    pub key: KuiItemKey,
}

pub(super) enum RuntimeNode {
    Scope {
        name: String,
        own_style: bool,
        props: Vec<crate::KuiProp>,
        inputs: BTreeMap<String, crate::KuiPropSource>,
        child: Box<RuntimeNode>,
        bindings: Vec<RuntimeBinding>,
        error: Option<String>,
    },
    Caller {
        children: Vec<RuntimeNode>,
        style: Option<Handle<bevy_flair::prelude::StyleSheet>>,
        styled: Vec<Entity>,
    },
    Slot {
        children: Vec<RuntimeNode>,
    },
    Visual {
        entity: Entity,
        bindings: Vec<RuntimeBinding>,
        children: Vec<RuntimeNode>,
        order: Vec<Entity>,
        dynamic_children: bool,
    },
    Repeat {
        world_space: bool,
        interactive: bool,
        items: String,
        alias: String,
        key: String,
        template: Box<KuiNode>,
        rows: BTreeMap<KuiItemKey, RuntimeRow>,
        cache: Box<RepeatCache>,
        order: Vec<KuiItemKey>,
        error: Option<String>,
    },
}

pub(super) struct RuntimeRow {
    node: RuntimeNode,
    index: usize,
}

#[derive(Default)]
pub(super) struct RepeatCache {
    dependencies: Option<Vec<(String, Option<KuiValue>)>>,
    previous_collection: Option<std::sync::Arc<KuiValue>>,
}

impl RuntimeNode {
    pub(super) fn root_inherits_stylesheet(&self) -> bool {
        match self {
            Self::Scope {
                own_style, child, ..
            } => !own_style && child.root_inherits_stylesheet(),
            Self::Visual { .. } => true,
            _ => false,
        }
    }

    pub(super) fn entity(&self) -> Option<Entity> {
        match self {
            Self::Scope { child, .. } => child.entity(),
            Self::Caller { .. } | Self::Slot { .. } => None,
            Self::Visual { entity, .. } => Some(*entity),
            Self::Repeat { .. } => None,
        }
    }

    pub(super) fn bindings<'a>(&'a mut self, result: &mut Vec<&'a mut RuntimeBinding>) {
        match self {
            Self::Scope {
                child, bindings, ..
            } => {
                child.bindings(result);
                result.extend(bindings);
            }
            Self::Caller { children, .. } | Self::Slot { children } => {
                for child in children {
                    child.bindings(result);
                }
            }
            Self::Visual {
                bindings, children, ..
            } => {
                result.extend(bindings);
                for child in children {
                    child.bindings(result);
                }
            }
            Self::Repeat { rows, .. } => {
                for row in rows.values_mut() {
                    row.node.bindings(result);
                }
            }
        }
    }

    fn can_change_roots(&self) -> bool {
        match self {
            Self::Repeat { .. } => true,
            Self::Scope { child, .. } => child.can_change_roots(),
            Self::Caller { children, .. } | Self::Slot { children } => {
                children.iter().any(Self::can_change_roots)
            }
            Self::Visual { .. } => false,
        }
    }

    fn entities(&self, result: &mut Vec<Entity>) {
        match self {
            Self::Scope { child, .. } => child.entities(result),
            Self::Caller { children, .. } | Self::Slot { children } => {
                for child in children {
                    child.entities(result);
                }
            }
            Self::Visual { entity, .. } => result.push(*entity),
            Self::Repeat { rows, order, .. } => {
                for key in order {
                    rows[key].node.entities(result);
                }
            }
        }
    }

    fn inherited_entities(&self, result: &mut Vec<Entity>) {
        match self {
            Self::Visual { entity, .. } => result.push(*entity),
            Self::Scope {
                own_style: false,
                child,
                ..
            } => child.inherited_entities(result),
            Self::Repeat { rows, order, .. } => {
                for key in order {
                    rows[key].node.inherited_entities(result);
                }
            }
            Self::Slot { children } => {
                for child in children {
                    child.inherited_entities(result);
                }
            }
            Self::Scope { .. } | Self::Caller { .. } => {}
        }
    }
}

/// Identifies a component instance without introducing another layout box.
#[derive(Component, Clone, Debug)]
pub(crate) struct KuiComponentInstance {
    #[cfg(test)]
    pub source: String,
}

pub(super) fn reconcile(
    mut views: Query<(Entity, &mut KuiView)>,
    assets: Res<Assets<KuiAsset>>,
    mut commands: Commands,
) {
    for (entity, mut view) in &mut views {
        if view.status != KuiViewStatus::Ready || !view.is_changed() {
            continue;
        }
        if let Some(error) = view.data.keys().find_map(|name| {
            crate::expression::environment()
                .validate_input_name(name)
                .err()
        }) {
            view.status = KuiViewStatus::Failed(error.to_string());
            continue;
        }
        let Some(asset) = assets.get(&view.asset) else {
            continue;
        };
        // Preserve the external input tick: evaluating bindings must not make
        // this same view dirty again on the next update.
        let view = view.bypass_change_detection();
        view.bindings_pending = true;
        view.expressions.begin();
        for tree in &mut view.tree {
            reconcile_node(
                tree,
                Some(entity),
                entity,
                asset,
                &view.data,
                &BTreeMap::new(),
                &[],
                &mut view.expressions,
                0,
                &mut commands,
            );
        }
        view.expressions.finish();
    }
}

fn reconcile_node(
    node: &mut RuntimeNode,
    parent: Option<Entity>,
    view: Entity,
    asset: &KuiAsset,
    data: &BTreeMap<String, KuiValue>,
    aliases: &BTreeMap<String, KuiValue>,
    callers: &[(
        &BTreeMap<String, KuiValue>,
        &BTreeMap<String, KuiValue>,
        usize,
    )],
    expressions: &mut crate::expression::Evaluator,
    scope: usize,
    commands: &mut Commands,
) {
    match node {
        RuntimeNode::Scope {
            name,
            props,
            inputs,
            child,
            bindings,
            error,
            ..
        } => {
            prepare_bindings(bindings, data, aliases, expressions, scope);
            let mut values = BTreeMap::new();
            let mut diagnostic = None;
            for prop in props {
                let value = match inputs.get(&prop.name) {
                    Some(crate::KuiPropSource::Binding(path)) => {
                        expressions.evaluate(scope, path, data, aliases)
                    }
                    Some(crate::KuiPropSource::Literal(value)) => prop.kind.literal(value).ok(),
                    None => prop
                        .default
                        .as_ref()
                        .and_then(|value| prop.kind.literal(value).ok()),
                };
                if let Some(value) = value.filter(|v| prop.kind.accepts(v)) {
                    values.insert(prop.name.clone(), value);
                } else {
                    diagnostic = Some(format!(
                        "component `{name}` prop `{}` requires {:?}",
                        prop.name, prop.kind
                    ));
                }
            }
            if diagnostic != *error {
                if let Some(message) = &diagnostic {
                    warn!("KUI view `{}`: {message}", asset.definition.name);
                }
                *error = diagnostic;
            }
            let local = BTreeMap::from([("props".into(), KuiValue::Record(values))]);
            let mut stack = callers.to_vec();
            stack.push((data, aliases, scope));
            let local_scope = expressions.enter();
            reconcile_node(
                child,
                parent,
                view,
                asset,
                &local,
                &BTreeMap::new(),
                &stack,
                expressions,
                local_scope,
                commands,
            );
        }
        RuntimeNode::Caller {
            children,
            style,
            styled,
        } => {
            let (data, aliases, scope) = callers.last().copied().expect("compiled slot caller");
            for child in children.iter_mut() {
                reconcile_node(
                    child,
                    parent,
                    view,
                    asset,
                    data,
                    aliases,
                    &callers[..callers.len() - 1],
                    expressions,
                    scope,
                    commands,
                );
            }
            let mut roots = vec![];
            for child in children {
                child.inherited_entities(&mut roots);
            }
            for entity in &roots {
                if !styled.contains(entity) {
                    commands.entity(*entity).insert(
                        style
                            .as_ref()
                            .map(|handle| bevy_flair::prelude::Styled::new(handle.clone()))
                            .unwrap_or(bevy_flair::prelude::Styled::Block),
                    );
                }
            }
            *styled = roots;
        }
        RuntimeNode::Slot { children } => {
            for child in children {
                reconcile_node(
                    child,
                    parent,
                    view,
                    asset,
                    data,
                    aliases,
                    callers,
                    expressions,
                    scope,
                    commands,
                );
            }
        }
        RuntimeNode::Visual {
            entity,
            bindings,
            children,
            order,
            dynamic_children,
        } => {
            prepare_bindings(bindings, data, aliases, expressions, scope);
            for child in children.iter_mut() {
                reconcile_node(
                    child,
                    Some(*entity),
                    view,
                    asset,
                    data,
                    aliases,
                    callers,
                    expressions,
                    scope,
                    commands,
                );
            }
            if *dynamic_children {
                let mut next = vec![];
                for child in children {
                    child.entities(&mut next);
                }
                if *order != next {
                    commands.entity(*entity).replace_children(&next);
                    crate::state_tree::invalidate_focus(commands, view);
                    *order = next;
                }
            }
        }
        RuntimeNode::Repeat {
            world_space,
            interactive,
            items,
            alias,
            key,
            template,
            rows,
            cache,
            order,
            error,
        } => {
            let collection = expressions
                .evaluate_shared(scope, items, data, aliases)
                .unwrap_or_else(|| std::sync::Arc::new(KuiValue::Nil));
            let next = match keys(&collection, key, expressions) {
                Ok(keys) => {
                    *error = None;
                    keys
                }
                Err(message) => {
                    if error.as_ref() != Some(&message) {
                        warn!("KUI view `{}`: {message}", asset.definition.name);
                        *error = Some(message);
                    }
                    vec![]
                }
            };
            let retained = next.iter().collect::<BTreeSet<_>>();
            rows.retain(|key, row| {
                if retained.contains(key) {
                    true
                } else {
                    commands
                        .entity(row.node.entity().expect("visual template"))
                        .despawn();
                    false
                }
            });
            let RepeatCache {
                dependencies,
                previous_collection,
            } = &mut **cache;
            let external_changed = match dependencies {
                Some(dependencies) => {
                    let mut changed = false;
                    for (name, previous) in dependencies {
                        let value = aliases.get(name).or_else(|| data.get(name));
                        if previous.as_ref() != value {
                            *previous = value.cloned();
                            changed = true;
                        }
                    }
                    changed
                }
                None => true,
            };
            for (index, key) in next.iter().enumerate() {
                let row = rows.entry(key.clone()).or_insert_with(|| {
                    let row = spawn(
                        commands,
                        parent,
                        view,
                        template,
                        asset,
                        *world_space,
                        *interactive,
                    );
                    #[cfg(test)]
                    commands
                        .entity(row.entity().expect("visual template"))
                        .insert(KuiRepeatItem { key: key.clone() });
                    RuntimeRow {
                        node: row,
                        index: usize::MAX,
                    }
                });
                let KuiValue::Sequence(values) = collection.as_ref() else {
                    unreachable!("validated collection");
                };
                let previous = previous_collection
                    .as_deref()
                    .and_then(|value| match value {
                        KuiValue::Sequence(values) => values.get(row.index),
                        _ => None,
                    });
                row.index = index;
                if !external_changed && previous == Some(&values[index]) {
                    continue;
                }
                let mut row_aliases = aliases.clone();
                row_aliases.insert(alias.clone(), values[index].clone());
                // A leaf with one binding cannot fan out an expression. Reuse
                // its program without allocating a result cache per row.
                let row_scope = if matches!(&row.node, RuntimeNode::Visual { bindings, children, .. } if bindings.len() <= 1 && children.is_empty())
                {
                    crate::expression::Evaluator::UNCACHED_SCOPE
                } else {
                    expressions.enter()
                };
                reconcile_node(
                    &mut row.node,
                    parent,
                    view,
                    asset,
                    data,
                    &row_aliases,
                    callers,
                    expressions,
                    row_scope,
                    commands,
                );
            }
            if *order != next {
                crate::state_tree::invalidate_focus(commands, view);
            }
            *order = next;
            // Reuse the evaluated collection itself for row comparisons; don't
            // clone a separate snapshot (or a persistent alias map per row).
            *previous_collection = dependencies.as_ref().map(|_| collection);
        }
    }
}

fn prepare_bindings(
    bindings: &mut [RuntimeBinding],
    data: &BTreeMap<String, KuiValue>,
    aliases: &BTreeMap<String, KuiValue>,
    expressions: &mut crate::expression::Evaluator,
    scope: usize,
) {
    for binding in bindings {
        binding.values.clear();
        let mut evaluate = |path: &str| {
            binding
                .values
                .push(expressions.evaluate(scope, path, data, aliases));
        };
        match &binding.operation {
            super::BindingOperation::TextTemplate { paths, .. } => {
                for path in paths {
                    evaluate(path);
                }
            }
            super::BindingOperation::Meter { current, maximum } => {
                evaluate(current);
                evaluate(maximum);
            }
            _ => evaluate(&binding.path),
        }
    }
}

fn keys(
    collection: &KuiValue,
    key: &str,
    expressions: &mut crate::expression::Evaluator,
) -> Result<Vec<KuiItemKey>, String> {
    let KuiValue::Sequence(values) = collection else {
        return Err("collection must be a sequence of records".into());
    };
    let mut seen = BTreeSet::new();
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let KuiValue::Record(record) = value else {
                return Err(format!("collection item {index} is not a record"));
            };
            let key = match expressions.evaluate(
                crate::expression::Evaluator::UNCACHED_SCOPE,
                key,
                record,
                &BTreeMap::new(),
            ) {
                Some(KuiValue::String(value)) if !value.is_empty() => KuiItemKey::String(value),
                Some(KuiValue::Integer(value)) => KuiItemKey::Integer(value),
                _ => {
                    return Err(format!(
                        "collection item {index} requires a nonempty string or integer key `{key}`"
                    ));
                }
            };
            if !seen.insert(key.clone()) {
                return Err(format!("collection has duplicate key {key:?}"));
            }
            Ok(key)
        })
        .collect()
}

pub(super) fn spawn(
    commands: &mut Commands,
    parent: Option<Entity>,
    view: Entity,
    node: &KuiNode,
    asset: &KuiAsset,
    world_space: bool,
    interactive: bool,
) -> RuntimeNode {
    let interactive = interactive
        || node
            .attributes
            .keys()
            .any(|key| crate::interaction::is_event_attribute(key));
    if let KuiNodeKind::Scope {
        name,
        props,
        inputs,
        template,
        style,
    } = &node.kind
    {
        let child = spawn(
            commands,
            parent,
            view,
            template,
            asset,
            world_space,
            interactive,
        );
        let entity = child.entity().expect("component visual root");
        super::initialize_scope_bindings(&mut commands.entity(entity), &node.bindings, world_space);
        super::apply_element_attributes(&mut commands.entity(entity), view, node, world_space);
        commands.entity(entity).insert(KuiComponentInstance {
            #[cfg(test)]
            source: name.clone(),
        });
        if let Some(path) = style {
            let index = asset
                .component_style_paths
                .iter()
                .position(|s| s == path)
                .expect("compiled stylesheet");
            commands
                .entity(entity)
                .insert(bevy_flair::prelude::Styled::new(
                    asset.component_style_handles[index].clone(),
                ));
        }
        return RuntimeNode::Scope {
            name: name.clone(),
            own_style: style.is_some(),
            props: props.clone(),
            inputs: inputs.clone(),
            child: Box::new(child),
            bindings: super::runtime_bindings(entity, &node.bindings),
            error: None,
        };
    }
    if let KuiNodeKind::Caller { children, style } = &node.kind {
        let children: Vec<_> = children
            .iter()
            .map(|node| {
                spawn(
                    commands,
                    parent,
                    view,
                    node,
                    asset,
                    world_space,
                    interactive,
                )
            })
            .collect();
        let handle = style.as_ref().and_then(|path| {
            asset
                .component_style_paths
                .iter()
                .position(|s| s == path)
                .map(|index| asset.component_style_handles[index].clone())
                .or_else(|| {
                    asset
                        .definition
                        .style
                        .as_ref()
                        .filter(|s| &s.src == path)
                        .and(asset.style_sheet.clone())
                })
        });
        return RuntimeNode::Caller {
            children,
            style: handle,
            styled: vec![],
        };
    }
    if let KuiNodeKind::Slot { children, .. } = &node.kind {
        return RuntimeNode::Slot {
            children: children
                .iter()
                .map(|node| {
                    spawn(
                        commands,
                        parent,
                        view,
                        node,
                        asset,
                        world_space,
                        interactive,
                    )
                })
                .collect(),
        };
    }
    if let KuiNodeKind::Repeat {
        items,
        alias,
        key,
        template,
    } = &node.kind
    {
        return RuntimeNode::Repeat {
            world_space,
            interactive,
            items: items.clone(),
            alias: alias.clone(),
            key: key.clone(),
            template: template.clone(),
            rows: BTreeMap::new(),
            cache: Box::new(RepeatCache {
                dependencies: crate::path::external_roots(template, std::slice::from_ref(alias))
                    .map(|roots| roots.into_iter().map(|root| (root, None)).collect()),
                previous_collection: None,
            }),
            order: vec![],
            error: None,
        };
    }
    let (entity, bindings) = spawn_node(commands, parent, view, node, asset, world_space);
    if world_space && interactive {
        commands.entity(entity).insert(Pickable::default());
    }
    let children: Vec<_> = match &node.kind {
        KuiNodeKind::Node { children } | KuiNodeKind::Button { children, .. } => children
            .iter()
            .map(|child| {
                spawn(
                    commands,
                    Some(entity),
                    view,
                    child,
                    asset,
                    world_space,
                    interactive,
                )
            })
            .collect(),
        _ => vec![],
    };
    let dynamic_children = children.iter().any(RuntimeNode::can_change_roots);
    RuntimeNode::Visual {
        entity,
        bindings,
        children,
        order: vec![],
        dynamic_children,
    }
}
