use super::{
    Backend, Manifest, Pending, RecordOwner, Records, Role, SaveBundle, SaveHandle, SaveInfo,
    SaveStats, Storage, StoredSave, WriteMode,
};
use crate::{Error, SavePlugin};
use js_sys::{Array, Reflect, Uint8Array};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    future::Future,
    time::{Duration, UNIX_EPOCH},
};
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};
use wasm_bindgen_futures::spawn_local;

#[wasm_bindgen(module = "/src/backend/indexeddb.js")]
extern "C" {
    #[wasm_bindgen(catch)]
    async fn list_saves(database: &str, collection: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch)]
    async fn read_save(database: &str, collection: &str, handle: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch)]
    async fn commit_save(
        database: &str,
        collection: &str,
        handle: &str,
        metadata: &str,
        payloads: Array,
    ) -> Result<(), JsValue>;
}

impl SavePlugin {
    pub fn indexeddb(database: impl Into<String>, collection: impl Into<String>) -> Self {
        let database = database.into();
        let collection = collection.into();
        Self::new(move || {
            Storage(Box::new(IndexedDb {
                database: database.clone(),
                collection: collection.clone(),
            }))
        })
    }
}

#[derive(Clone)]
struct IndexedDb {
    database: String,
    collection: String,
}

impl Backend for IndexedDb {
    fn commit(&self, handle: SaveHandle, bundle: SaveBundle) -> Result<Pending<SaveStats>, Error> {
        let backend = self.clone();
        Ok(submit(async move { backend.write(handle, bundle).await }))
    }

    fn read(&self, handle: SaveHandle) -> Result<Pending<StoredSave>, Error> {
        let backend = self.clone();
        Ok(submit(async move {
            let value = read_save(&backend.database, &backend.collection, &key(handle))
                .await
                .map_err(browser_error)?;
            decode_save(value)
        }))
    }

    fn list(&self) -> Result<Pending<Vec<SaveInfo>>, Error> {
        let backend = self.clone();
        Ok(submit(async move {
            let value = list_saves(&backend.database, &backend.collection)
                .await
                .map_err(browser_error)?;
            decode_list(value)
        }))
    }
}

impl IndexedDb {
    async fn write(&self, handle: SaveHandle, bundle: SaveBundle) -> Result<SaveStats, Error> {
        let generation = bundle
            .expected_generation
            .checked_add(1)
            .ok_or_else(|| Error::InvalidData("generation exhausted".into()))?;
        let stats = SaveStats {
            generation,
            entities: bundle.entity_count,
            upserts: bundle.batch.upserts.len(),
            bytes: bundle
                .batch
                .upserts
                .iter()
                .map(|record| record.payload.len())
                .sum(),
        };
        let metadata = serde_json::to_string(&Write {
            expected: bundle.expected_generation.to_string(),
            manifest: bundle
                .manifest
                .iter()
                .map(|(key, role)| (key.as_str(), u8::from(*role == Role::Component)))
                .collect(),
            replace: bundle.mode != WriteMode::Incremental,
            rewrite: bundle.mode == WriteMode::ReplaceAll,
            added: bundle
                .added_entities
                .iter()
                .map(ToString::to_string)
                .collect(),
            removed: bundle
                .removed_entities
                .iter()
                .map(ToString::to_string)
                .collect(),
            deletes: bundle
                .batch
                .deletes
                .iter()
                .map(|(owner, key)| (owner_key(*owner), *key))
                .collect(),
            upserts: bundle
                .batch
                .upserts
                .iter()
                .map(|record| {
                    (
                        owner_key(record.owner),
                        record.key,
                        record.references.iter().map(ToString::to_string).collect(),
                    )
                })
                .collect(),
        })?;
        let payloads = bundle
            .batch
            .upserts
            .into_iter()
            .map(|record| Uint8Array::from(record.payload.as_slice()))
            .collect();
        commit_save(
            &self.database,
            &self.collection,
            &key(handle),
            &metadata,
            payloads,
        )
        .await
        .map_err(browser_error)?;
        Ok(stats)
    }
}

#[derive(Serialize)]
struct Write<'a> {
    expected: String,
    manifest: Vec<(&'a str, u8)>,
    replace: bool,
    rewrite: bool,
    added: Vec<String>,
    removed: Vec<String>,
    deletes: Vec<(String, &'static str)>,
    upserts: Vec<(String, &'static str, Vec<String>)>,
}

#[derive(Deserialize)]
struct Read {
    generation: String,
    manifest: Vec<(String, u8)>,
    entities: Vec<String>,
    records: Vec<(String, String)>,
}

fn submit<T: 'static>(operation: impl Future<Output = Result<T, Error>> + 'static) -> Pending<T> {
    let (send, pending) = Pending::channel();
    spawn_local(async move {
        let _ = send.send(operation.await);
    });
    pending
}

fn key(handle: SaveHandle) -> String {
    format!("{:032x}", handle.0)
}

fn owner_key(owner: RecordOwner) -> String {
    owner.entity_id().unwrap_or(0).to_string()
}

fn decode_json<T: serde::de::DeserializeOwned>(value: JsValue) -> Result<T, Error> {
    let data = value
        .as_string()
        .ok_or_else(|| Error::InvalidData("expected IndexedDB metadata".into()))?;
    Ok(serde_json::from_str(&data)?)
}

fn decode_list(value: JsValue) -> Result<Vec<SaveInfo>, Error> {
    let entries = decode_json::<Vec<(String, u64)>>(value)?;
    let mut saves = Vec::with_capacity(entries.len());
    for (handle, millis) in entries {
        let id = u128::from_str_radix(&handle, 16)
            .map_err(|_| Error::InvalidData("invalid save handle".into()))?;
        let id = SaveHandle(id);
        if key(id) != handle {
            return Err(Error::InvalidData("invalid save handle".into()));
        }
        let created_at = UNIX_EPOCH
            .checked_add(Duration::from_millis(millis))
            .ok_or_else(|| Error::InvalidData("creation timestamp out of range".into()))?;
        saves.push(SaveInfo {
            handle: id,
            created_at,
        });
    }
    saves.sort_unstable_by_key(|save| save.handle);
    Ok(saves)
}

fn decode_save(value: JsValue) -> Result<StoredSave, Error> {
    let result = value.dyn_into::<Array>().map_err(|_| Error::Format)?;
    let read = decode_json::<Read>(result.get(0))?;
    let payloads = result
        .get(1)
        .dyn_into::<Array>()
        .map_err(|_| Error::Format)?;
    if payloads.length() as usize != read.records.len() {
        return Err(Error::InvalidData("missing record payload".into()));
    }
    let generation = read
        .generation
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| Error::InvalidData("invalid save generation".into()))?;
    let mut entities = Vec::with_capacity(read.entities.len());
    let mut ids = HashSet::with_capacity(read.entities.len());
    for id in read.entities {
        let id = parse_id(&id)?;
        if id == 0 || !ids.insert(id) {
            return Err(Error::InvalidData("invalid persistent entity ID".into()));
        }
        entities.push(id);
    }
    let mut manifest = Manifest::new();
    for (key, role) in read.manifest {
        let role = match role {
            0 => Role::Global,
            1 => Role::Component,
            _ => return Err(Error::InvalidData("invalid persistent type role".into())),
        };
        if manifest.insert(key, role).is_some() {
            return Err(Error::Manifest);
        }
    }
    let mut records = Records::new();
    for ((id, key), payload) in read.records.into_iter().zip(payloads.iter()) {
        let id = parse_id(&id)?;
        let owner = if id == 0 {
            RecordOwner::GLOBAL
        } else if ids.contains(&id) {
            RecordOwner::entity(id)
        } else {
            return Err(Error::InvalidData(
                "record references a missing entity".into(),
            ));
        };
        let bytes = payload
            .dyn_into::<Uint8Array>()
            .map_err(|_| Error::Format)?;
        records
            .entry(key)
            .or_default()
            .push((owner, bytes.to_vec()));
    }
    Ok(StoredSave {
        generation,
        entities,
        records,
        manifest,
    })
}

fn parse_id(id: &str) -> Result<i64, Error> {
    id.parse::<i64>()
        .ok()
        .filter(|value| *value >= 0 && value.to_string() == id)
        .ok_or_else(|| Error::InvalidData("invalid persistent entity ID".into()))
}

fn browser_error(value: JsValue) -> Error {
    let property = |name| {
        Reflect::get(&value, &JsValue::from_str(name))
            .ok()
            .and_then(|value| value.as_string())
    };
    let name = property("name").unwrap_or_else(|| "IndexedDB".into());
    let message = property("message")
        .or_else(|| value.as_string())
        .unwrap_or_else(|| "unknown error".into());
    match name.as_str() {
        "SaveNotFound" => Error::NotFound,
        "SaveFormat" => Error::Format,
        "SaveManifest" => Error::Manifest,
        "SaveInvalidData" => Error::InvalidData(message),
        "SaveConflict" => {
            let generation = |name| property(name).and_then(|value| value.parse::<u64>().ok());
            match (generation("expected"), generation("actual")) {
                (Some(expected), Some(actual)) => Error::Conflict { expected, actual },
                _ => Error::InvalidData(message),
            }
        }
        _ => Error::Io(Box::new(std::io::Error::other(format!(
            "{name}: {message}"
        )))),
    }
}
