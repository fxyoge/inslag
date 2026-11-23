use crate::{
    SaveHandle,
    backend::{Batch, Manifest, Pending, Records, SaveBundle, Storage, StoredSave, WriteMode},
    entity,
    identity::{Identities, Identity, IdentityChanges},
    registration::Registry,
};
use bevy_ecs::{
    change_detection::Tick,
    entity::{Entity, EntityHashMap},
    prelude::*,
};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("another persistence operation is in progress")]
    Busy,
    #[error("session is not ready to save")]
    NotReady,
    #[error("no save session is open")]
    Closed,
    #[error("close the current save session first")]
    AlreadyOpen,
    #[error("save does not exist")]
    NotFound,
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
    #[error("save storage: {0}")]
    Io(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("unrecognized database or unsupported save format")]
    Format,
    #[error("save schema revision {found} is newer than supported revision {supported}")]
    SchemaTooNew { found: u32, supported: u32 },
    #[error("save migration {revision} ({name}) failed: {source}")]
    Migration {
        revision: u32,
        name: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
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
    /// Payload only; excludes storage overhead.
    pub bytes: usize,
}

pub(crate) struct Saving {
    pub(crate) pending: Pending<SaveStats>,
    tick: Tick,
    identities: IdentityChanges,
}

pub(crate) fn begin_save(world: &mut World, handle: SaveHandle) -> Result<Saving, Error> {
    world.flush();
    world.resource_scope(|world, mut registry: Mut<Registry>| {
        let bundle = capture(world, &mut registry)?;
        let tick = world.read_change_tick();
        let pending = world.resource::<Storage>().commit(handle, bundle)?;
        for entry in &mut registry.entries {
            entry.codec.begin_commit(world);
        }
        let identities = world.resource_mut::<Identities>().begin_commit();
        // Edits after capture must not share its change tick.
        world.increment_change_tick();
        Ok(Saving {
            pending,
            tick,
            identities,
        })
    })
}

impl Saving {
    pub(crate) fn finish(self, world: &mut World, result: &Result<SaveStats, Error>) {
        let committed = result.is_ok();
        world.resource_scope(|world, mut registry: Mut<Registry>| {
            for entry in &mut registry.entries {
                entry.codec.finish_commit(world, self.tick, committed);
            }
            if let Ok(stats) = result {
                registry.baseline = true;
                registry.generation = Some(stats.generation);
                registry.rewrite_manifest = false;
            }
            world
                .resource_mut::<Identities>()
                .finish_commit(self.identities, committed);
        });
    }
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
        expected_generation: registry.generation.ok_or(Error::Closed)?,
        manifest: registry
            .entries
            .iter()
            .map(|entry| (entry.key.to_owned(), entry.role))
            .collect(),
        mode: if registry.rewrite_manifest {
            WriteMode::ReplaceAll
        } else if baseline {
            WriteMode::Incremental
        } else {
            WriteMode::ReplaceRecords
        },
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

pub(crate) fn start_fresh(world: &mut World) {
    clear_scope(world);
    let mut registry = world.resource_mut::<Registry>();
    registry.started = true;
    registry.generation = Some(0);
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
        registry.generation = None;
        registry.rewrite_manifest = false;
    });
}

pub(crate) fn restore(world: &mut World, saved: &StoredSave) -> Result<(), Error> {
    world.flush();
    world.resource_scope(|world, mut registry: Mut<Registry>| {
        registry.started = true;
        check_load_manifest(&saved.manifest, &registry)?;
        restore_records(
            world,
            &mut registry,
            &saved.entities,
            &saved.records,
            &saved.manifest,
        )?;
        registry.baseline = false;
        registry.generation = Some(saved.generation);
        registry.rewrite_manifest = saved.manifest.len() != registry.entries.len();
        world.increment_change_tick();
        Ok(())
    })
}

fn check_load_manifest(actual: &Manifest, registry: &Registry) -> Result<(), Error> {
    for (key, role) in actual {
        if !registry
            .entries
            .iter()
            .any(|entry| entry.key == key && entry.role == *role)
        {
            return Err(Error::Manifest);
        }
    }
    Ok(())
}

fn restore_records(
    world: &mut World,
    registry: &mut Registry,
    saved_ids: &[i64],
    records: &Records,
    manifest: &Manifest,
) -> Result<(), Error> {
    let next = saved_ids
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| Error::InvalidData("persistent entity IDs exhausted".into()))?;
    let mut actual: HashMap<_, _> = saved_ids
        .iter()
        .copied()
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
                            .decode(world, records.get(entry.key).map_or(&[], Vec::as_slice))?
                    } else {
                        entry.codec.decode_missing(world)?
                    });
                }
                if records.keys().any(|key| !manifest.contains_key(key)) {
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
    use std::time::{Duration, Instant, UNIX_EPOCH};

    #[derive(Component, Serialize, Deserialize)]
    struct Value(u32);

    #[test]
    fn commit_uses_captured_records_and_entity_membership() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new();
        app.add_plugins(SavePlugin::sqlite(directory.path()))
            .persist(component::<Value>("value").capture(Capture::Dirty));
        let world = app.world_mut();
        let handle = SaveHandle(1);
        start_fresh(world);
        let removed = world.spawn(Value(1)).id();
        let mut saving = begin_save(world, handle).unwrap();
        let result = complete(&mut saving.pending);
        saving.finish(world, &result);
        assert_eq!(result.unwrap().generation, 1);
        world.despawn(removed);
        let added = world.spawn(Value(2)).id();
        let bundle = world
            .resource_scope(|world, mut registry: Mut<Registry>| capture(world, &mut registry))
            .unwrap();

        world.despawn(added);
        world.spawn(Value(3));

        let storage = world.resource::<Storage>();
        let stats = complete(&mut storage.commit(handle, bundle).unwrap()).unwrap();
        assert_eq!((stats.generation, stats.entities, stats.upserts), (2, 1, 1));
        let saved = complete(&mut storage.read(handle).unwrap()).unwrap();
        let [(owner, bytes)] = saved.records["value"].as_slice() else {
            panic!("expected one saved value");
        };
        assert_eq!(saved.entities, [owner.entity_id().unwrap()]);
        assert_eq!(serde_json::from_slice::<Value>(bytes).unwrap().0, 2);
    }

    #[test]
    fn aborted_first_commit_does_not_publish_creation_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let mut app = App::new();
        app.add_plugins(SavePlugin::sqlite(directory.path()))
            .persist(component::<Value>("value"));
        let world = app.world_mut();
        let handle = SaveHandle(1);
        start_fresh(world);
        world.spawn(Value(1));
        let mut bundle = world
            .resource_scope(|world, mut registry: Mut<Registry>| capture(world, &mut registry))
            .unwrap();
        bundle.batch.upserts[0].references.push(i64::MAX);
        let storage = world.resource::<Storage>();
        assert!(complete(&mut storage.commit(handle, bundle).unwrap()).is_err());
        assert!(complete(&mut storage.list().unwrap()).unwrap().is_empty());

        let mut saving = begin_save(world, handle).unwrap();
        let result = complete(&mut saving.pending);
        saving.finish(world, &result);
        assert_eq!(result.unwrap().generation, 1);
        let saves = complete(&mut world.resource::<Storage>().list().unwrap()).unwrap();
        assert_eq!(saves.len(), 1);
        assert_eq!(saves[0].handle, handle);
        assert!(saves[0].created_at > UNIX_EPOCH);
    }

    fn complete<T>(pending: &mut Pending<T>) -> Result<T, Error> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(result) = pending.poll() {
                return result;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
    }
}
