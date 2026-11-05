use super::*;
use crate::persistence::{
    Apply, Error, TreeSnapshot, Value, entity_id, invalid, loaded_entity, query_all,
};
use bevy_ecs::{prelude::Without, world::World};

pub(crate) fn entities(world: &mut World) -> Vec<Entity> {
    query_all::<
        Entity,
        (
            With<StateTreeInstance>,
            Without<crate::spawn_activity::SpawnedStateTree>,
            With<crate::PersistStateTree>,
        ),
    >(world)
    .iter(world)
    .collect()
}

pub(crate) fn capture(world: &mut World) -> Result<Vec<TreeSnapshot>, Error> {
    let mut trees = Vec::new();
    for (entity, instance) in query_all::<
        (Entity, &StateTreeInstance),
        (
            Without<crate::spawn_activity::SpawnedStateTree>,
            With<crate::PersistStateTree>,
        ),
    >(world)
    .iter(world)
    {
        let definition = world
            .resource::<Assets<StateTreeDefinition>>()
            .get(instance.definition())
            .map(|asset| asset.name().to_string())
            .or_else(|| instance.definition().path().map(ToString::to_string))
            .ok_or_else(|| invalid("generated definition is unavailable"))?;
        trees.push(TreeSnapshot {
            owner: entity_id(entity)?,
            definition,
            path: instance.definition().path().map(ToString::to_string),
            data: instance
                .datamodel_values()
                .into_iter()
                .map(|(name, value)| (name, Value::from(&value)))
                .collect(),
        });
    }
    trees.sort_by_key(|tree| tree.owner);
    Ok(trees)
}

pub(crate) fn prepare(world: &mut World, trees: &[TreeSnapshot]) -> Result<Apply, Error> {
    let mut prepared = Vec::new();
    let mut owners = std::collections::HashSet::new();
    for tree in trees {
        let entity = loaded_entity(tree.owner)?;
        if !owners.insert(entity) {
            return Err(invalid("duplicate tree owner"));
        }
        let handle = if let Some(path) = &tree.path {
            world
                .resource::<AssetServer>()
                .load::<StateTreeDefinition>(path.clone())
        } else {
            let id = {
                let assets = world.resource::<Assets<StateTreeDefinition>>();
                let mut candidates = assets
                    .iter()
                    .filter(|(_, asset)| asset.name() == tree.definition);
                let id = candidates
                    .next()
                    .map(|(id, _)| id)
                    .ok_or_else(|| Error::AssetNotReady(tree.definition.clone()))?;
                if candidates.next().is_some() {
                    return Err(invalid("ambiguous generated definition"));
                }
                id
            };
            world
                .resource_mut::<Assets<StateTreeDefinition>>()
                .get_strong_handle(id)
                .ok_or_else(|| invalid("definition handle unavailable"))?
        };
        prepared.push((
            entity,
            StateTreeInstance::new(handle).with_preserved_data(
                tree.data
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone().into()))
                    .collect(),
            ),
        ));
    }
    Ok(Box::new(move |world| {
        for (entity, instance) in prepared {
            world
                .entity_mut(entity)
                .insert((instance, crate::PersistStateTree));
        }
    }))
}

pub(crate) fn clear(world: &mut World) {
    // Retain cancellation delivery so transient consumers release run-owned objects.
    for effect in world
        .resource::<StateTreeEffectRegistry>()
        .handlers_for_persistence()
    {
        effect.clear_messages(world);
    }
    world.resource_scope(|world, registry: Mut<StateTreeActivityRegistry>| {
        registry.discard_pending_starts(world)
    });
    let mut transient =
        query_all::<(), (With<StateTreeInstance>, Without<crate::PersistStateTree>)>(world);
    let batch = world
        .resource::<StateTreeEventIngress>()
        .batch
        .load(Ordering::Acquire);
    let mut event_dirty = EntityHashSet::default();
    for (entity, instance, inbox) in
        query_all::<(Entity, &StateTreeInstance, &StateTreeEventInbox), ()>(world).iter(world)
    {
        let mut events = inbox
            .events
            .lock()
            .expect("state-tree event inbox poisoned");
        if transient.get(world, entity).is_ok() {
            events.retain(|event| event.batch == batch && event.instance == instance.runtime_id());
            // The old world's broadcasts are dropped, so retained targets must
            // not keep positions into that discarded broadcast stream.
            for event in events.iter_mut() {
                event.broadcasts_before = 0;
            }
            if !events.is_empty() {
                event_dirty.insert(entity);
            }
        } else {
            events.clear();
        }
    }
    let mut activity_dirty = EntityHashSet::default();
    for (entity, instance, inbox) in
        query_all::<(Entity, &StateTreeInstance, &StateTreeActivityInbox), ()>(world).iter(world)
    {
        let mut outcomes = inbox
            .outcomes
            .lock()
            .expect("state-tree activity inbox poisoned");
        if transient.get(world, entity).is_ok() {
            outcomes.retain(|outcome| instance.accepts_activity_token(outcome.token));
            if !outcomes.is_empty() {
                activity_dirty.insert(entity);
            }
        } else {
            outcomes.clear();
        }
    }
    let mut ingress = world.resource_mut::<StateTreeEventIngress>();
    ingress
        .broadcasts
        .get_mut()
        .expect("state-tree broadcast inbox poisoned")
        .clear();
    *ingress.published_broadcasts.get_mut() = 0;
    *ingress
        .dirty_entities
        .get_mut()
        .expect("state-tree dirty entity set poisoned") = event_dirty;
    // Retained inbox entries carry this batch identity; resetting it would lose
    // their events on dispatch or the next write to the same inbox.
    let mut ingress = world.resource_mut::<StateTreeActivityIngress>();
    *ingress.dirty_hint.get_mut() = !activity_dirty.is_empty();
    *ingress
        .dirty
        .get_mut()
        .expect("state-tree activity dirty set poisoned") = activity_dirty;
    world.resource_scope(
        |world, mut timers: bevy_ecs::prelude::Mut<StateTreeTimers>| {
            timers.apply_staged();
            timers
                .active
                .retain(|_, timer| transient.get(world, timer.entity).is_ok());
            let timers = &mut *timers;
            timers.heap.retain(|Reverse(timer)| {
                timers.active.get(&timer.token)
                    == Some(&ActiveTimer {
                        deadline: timer.deadline,
                        entity: timer.entity,
                    })
            });
        },
    );
}
