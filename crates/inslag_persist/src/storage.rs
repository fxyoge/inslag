use crate::{
    LoadOutcome, entity,
    identity::{Identities, Identity},
    registration::{Batch, Registry},
};
use bevy_ecs::{
    entity::{Entity, EntityHashMap},
    prelude::*,
};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
};

mod sqlite;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("install SavePlugin before saving or loading")]
    MissingPlugin,
    #[error("another persistence operation is in progress")]
    Busy,
    #[error("session is not ready to save")]
    NotReady,
    #[error("session is closed; open a save path first")]
    Closed,
    #[error("encoding {key}: {source}")]
    Encode {
        key: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("decoding {key}: {source}")]
    Decode {
        key: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("load this save successfully before writing")]
    LoadRequired,
    #[error("save file: {0}")]
    Io(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("unrecognized database or unsupported save format")]
    Format,
    #[error("persistent type keys/roles do not match this save's manifest")]
    Manifest,
    #[error("save generation changed: expected {expected}, found {actual}; load before retrying")]
    Conflict { expected: u64, actual: u64 },
    #[error("invalid save data: {0}")]
    InvalidData(String),
    #[error("waiting for asset: {0}")]
    AssetNotReady(String),
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(Box::new(error))
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::InvalidData(error.to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveStats {
    pub generation: u64,
    pub entities: usize,
    /// Counts records, not entities; includes snapshot rewrites.
    pub upserts: usize,
    pub deletions: usize,
    /// Payload only; excludes SQL/index/WAL overhead.
    pub bytes: usize,
}

struct SaveBundle {
    expected_generation: Option<u64>,
    manifest: BTreeMap<String, i64>,
    replace_records: bool,
    rewrite_manifest: bool,
    added_entities: Vec<i64>,
    removed_entities: Vec<i64>,
    entity_count: usize,
    batch: Batch,
}

struct StoredSave {
    generation: u64,
    entities: Vec<i64>,
    records: BTreeMap<String, Vec<(i64, Vec<u8>)>>,
    manifest: BTreeMap<String, i64>,
}

trait Backend: Send + Sync {
    fn commit(&mut self, bundle: SaveBundle) -> Result<SaveStats, Error>;
    fn read(&mut self) -> Result<Option<StoredSave>, Error>;
}

#[derive(Default, Resource)]
pub(crate) struct Storage {
    backend: Option<Box<dyn Backend>>,
    blocked: bool,
}

impl Storage {
    pub(crate) fn sqlite(path: PathBuf) -> Self {
        Self {
            backend: Some(Box::new(sqlite::Sqlite::new(path))),
            blocked: false,
        }
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.backend.is_none()
    }

    pub(crate) fn close(&mut self) {
        self.backend = None;
    }

    fn backend(&mut self) -> Result<&mut (dyn Backend + 'static), Error> {
        self.backend.as_deref_mut().ok_or(Error::Closed)
    }
}

pub(crate) fn save(world: &mut World) -> Result<SaveStats, Error> {
    if !world.contains_resource::<Storage>() {
        return Err(Error::MissingPlugin);
    }
    if world.resource::<Storage>().is_closed() {
        return Err(Error::Closed);
    }
    if world.resource::<Storage>().blocked {
        return Err(Error::LoadRequired);
    }
    world.flush();
    world.resource_scope(|world, mut registry: Mut<Registry>| {
        let bundle = capture(world, &mut registry)?;
        let tick = world.read_change_tick();
        let stats = world.resource_mut::<Storage>().backend()?.commit(bundle)?;
        for entry in &mut registry.entries {
            entry.codec.acknowledge(world, tick);
        }
        registry.baseline = true;
        registry.generation = Some(stats.generation);
        registry.rewrite_manifest = false;
        world.resource_mut::<Identities>().acknowledge();
        // Direct World mutations after this call must not share the saved tick.
        world.increment_change_tick();
        Ok(stats)
    })
}

fn capture(world: &mut World, registry: &mut Registry) -> Result<SaveBundle, Error> {
    registry.started = true;
    for entry in &mut registry.entries {
        entry.codec.reconcile(world);
    }
    let mut batch = Batch::default();
    let baseline = registry.baseline;
    entity::saving(world.resource::<Identities>().entries.clone(), || {
        let mut contexts = Vec::new();
        for entry in &mut registry.entries {
            contexts.extend(entry.codec.capture(world, baseline, &mut batch)?);
        }
        Ok::<_, Error>(())
    })?;
    let ids = world.resource::<Identities>();
    let added_entities = if baseline {
        ids.added
            .iter()
            .map(|entity| ids.entries[entity].id)
            .collect()
    } else {
        ids.entries
            .values()
            .filter(|identity| identity.memberships > 0)
            .map(|identity| identity.id)
            .collect()
    };
    Ok(SaveBundle {
        expected_generation: registry.generation,
        manifest: registry
            .entries
            .iter()
            .map(|entry| (entry.key.to_owned(), entry.kind))
            .collect(),
        replace_records: !baseline,
        rewrite_manifest: registry.rewrite_manifest,
        added_entities,
        removed_entities: ids
            .removed
            .iter()
            .map(|entity| ids.entries[entity].id)
            .collect(),
        entity_count: ids.count,
        batch,
    })
}

pub(crate) fn load(world: &mut World) -> Result<LoadOutcome, Error> {
    if !world.contains_resource::<Storage>() {
        return Err(Error::MissingPlugin);
    }
    if world.resource::<Storage>().is_closed() {
        return Err(Error::Closed);
    }
    // Also guards against a panic during user deserialization/component hooks.
    world.resource_mut::<Storage>().blocked = true;
    let result = load_inner(world);
    if result.is_ok() {
        world.resource_mut::<Storage>().blocked = false;
    }
    result
}

pub(crate) fn can_save(world: &World) -> bool {
    let storage = world.resource::<Storage>();
    !storage.is_closed() && !storage.blocked && world.resource::<Registry>().generation.is_some()
}

pub(crate) fn clear_scope(world: &mut World) {
    world.flush();
    world.resource_scope(|world, mut registry: Mut<Registry>| {
        for entry in &mut registry.entries {
            entry.codec.reconcile(world);
        }
        let entities: Vec<_> = world
            .resource::<Identities>()
            .entries
            .iter()
            .filter(|(_, id)| id.memberships > 0)
            .map(|(entity, _)| *entity)
            .collect();
        for entity in entities {
            world.despawn(entity);
        }
        for entry in &registry.entries {
            entry.codec.clear(world);
        }
        for entry in &registry.entries {
            entry.codec.initialize(world);
        }
        world.insert_resource(Identities::default());
        for entry in &mut registry.entries {
            entry.codec.reset(world);
        }
        registry.baseline = false;
        registry.rewrite_manifest = false;
    });
}

fn load_inner(world: &mut World) -> Result<LoadOutcome, Error> {
    world.flush();
    world.resource_scope(|world, mut registry: Mut<Registry>| {
        registry.started = true;
        let Some(saved) = world.resource_mut::<Storage>().backend()?.read()? else {
            registry.baseline = false;
            registry.generation = Some(0);
            registry.rewrite_manifest = false;
            return Ok(LoadOutcome::Missing);
        };
        check_load_manifest(&saved.manifest, &registry)?;
        restore_records(
            world,
            &mut registry,
            saved.entities,
            saved.records,
            &saved.manifest,
        )?;
        registry.baseline = false;
        registry.generation = Some(saved.generation);
        registry.rewrite_manifest = saved.manifest.len() != registry.entries.len();
        world.increment_change_tick();
        Ok(LoadOutcome::Loaded)
    })
}

fn check_load_manifest(actual: &BTreeMap<String, i64>, registry: &Registry) -> Result<(), Error> {
    for (key, kind) in actual {
        if !registry
            .entries
            .iter()
            .any(|entry| entry.key == key && entry.kind == *kind)
        {
            return Err(Error::Manifest);
        }
    }
    Ok(())
}

fn restore_records(
    world: &mut World,
    registry: &mut Registry,
    saved_ids: Vec<i64>,
    mut records: BTreeMap<String, Vec<(i64, Vec<u8>)>>,
    manifest: &BTreeMap<String, i64>,
) -> Result<(), Error> {
    let next = saved_ids
        .last()
        .copied()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| Error::InvalidData("persistent entity IDs exhausted".into()))?;
    let mut actual: HashMap<_, _> = saved_ids
        .into_iter()
        .map(|id| (id, world.spawn_empty().id()))
        .collect();
    let (prepared, references) = entity::references(|| {
        entity::loading(
            Arc::new(actual.clone()),
            world.get_resource::<bevy_asset::AssetServer>().cloned(),
            || {
                let mut decoded = Vec::new();
                for entry in &registry.entries {
                    decoded.push(if manifest.contains_key(entry.key) {
                        entry
                            .codec
                            .decode(world, records.remove(entry.key).unwrap_or_default())?
                    } else {
                        entry.codec.decode_missing(world)?
                    });
                }
                if !records.is_empty() {
                    return Err(Error::InvalidData("unknown record type".into()));
                }
                Ok(decoded)
            },
        )
    });
    let decoded = match prepared {
        Ok(decoded) => decoded,
        Err(error) => {
            for entity in actual.values() {
                world.despawn(*entity);
            }
            return Err(error);
        }
    };
    let owned: std::collections::HashSet<_> = decoded
        .iter()
        .flat_map(|value| value.entities(&actual))
        .collect();
    let allocated: std::collections::HashSet<_> = actual.values().copied().collect();
    if !owned.is_subset(&allocated)
        || references
            .iter()
            .any(|id| !actual.get(id).is_some_and(|entity| owned.contains(entity)))
    {
        for entity in actual.values() {
            world.despawn(*entity);
        }
        return Err(Error::InvalidData(
            "reference to an entity without a restored owner".into(),
        ));
    }
    actual.retain(|_, entity| {
        if owned.contains(entity) {
            true
        } else {
            world.despawn(*entity);
            false
        }
    });
    for entry in &mut registry.entries {
        entry.codec.reconcile(world);
    }
    let old: Vec<_> = world
        .resource::<Identities>()
        .entries
        .iter()
        .filter(|(_, identity)| identity.memberships > 0)
        .map(|(entity, _)| *entity)
        .collect();
    world.resource_mut::<Identities>().suspended = true;
    for entity in old {
        world.despawn(entity);
    }
    world.flush();
    for entry in &registry.entries {
        entry.codec.clear(world);
    }
    let mut mapper: EntityHashMap<Entity> =
        actual.values().map(|entity| (*entity, *entity)).collect();
    for value in decoded {
        value.apply(world, &actual, &mut mapper);
    }
    world.flush();
    world.insert_resource(Identities {
        entries: Arc::new(
            actual
                .into_iter()
                .map(|(id, entity)| (entity, Identity { id, memberships: 0 }))
                .collect(),
        ),
        next,
        ..Default::default()
    });
    for entry in &mut registry.entries {
        entry.codec.reset(world);
    }
    registry.baseline = false;
    world.increment_change_tick();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppSaveExt, Capture, SavePlugin, component};
    use bevy_app::App;
    use serde::{Deserialize, Serialize};

    #[derive(Component, Serialize, Deserialize)]
    struct Value(u32);

    #[test]
    fn commit_uses_captured_records_and_entity_membership() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("save.sqlite");
        let mut app = App::new();
        app.add_plugins(SavePlugin::sqlite(path))
            .persist(component::<Value>("value").capture(Capture::Dirty));
        let world = app.world_mut();
        let removed = world.spawn(Value(1)).id();
        assert_eq!(save(world).unwrap().generation, 1);
        world.despawn(removed);
        let added = world.spawn(Value(2)).id();
        let bundle = world
            .resource_scope(|world, mut registry: Mut<Registry>| capture(world, &mut registry))
            .unwrap();

        world.despawn(added);
        world.spawn(Value(3));

        let mut storage = world.resource_mut::<Storage>();
        let backend = storage.backend().unwrap();
        let stats = backend.commit(bundle).unwrap();
        assert_eq!((stats.generation, stats.entities, stats.upserts), (2, 1, 1));
        let saved = backend.read().unwrap().unwrap();
        let [(id, bytes)] = saved.records["value"].as_slice() else {
            panic!("expected one saved value");
        };
        assert_eq!(saved.entities, [*id]);
        assert_eq!(serde_json::from_slice::<Value>(bytes).unwrap().0, 2);
    }
}
