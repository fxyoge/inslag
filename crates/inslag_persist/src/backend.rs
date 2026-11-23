use crate::{Error, SaveHandle, SaveInfo, SaveStats};
use bevy_ecs::prelude::Resource;
use std::{
    collections::BTreeMap,
    num::NonZeroI64,
    sync::{
        Mutex,
        mpsc::{self, Receiver, RecvError, Sender, TryRecvError},
    },
};

#[cfg(all(feature = "indexeddb", target_arch = "wasm32", target_os = "unknown"))]
mod indexeddb;
#[cfg(all(feature = "sqlite", not(target_family = "wasm")))]
mod sqlite;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Global,
    Component,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RecordOwner(Option<NonZeroI64>);

impl RecordOwner {
    pub(crate) const GLOBAL: Self = Self(None);

    pub(crate) fn entity(id: i64) -> Self {
        Self(Some(
            NonZeroI64::new(id).expect("persistent entity ID is nonzero"),
        ))
    }

    pub(crate) fn entity_id(self) -> Option<i64> {
        self.0.map(NonZeroI64::get)
    }
}

pub(crate) type Manifest = BTreeMap<String, Role>;
pub(crate) type Records = BTreeMap<String, Vec<(RecordOwner, Vec<u8>)>>;

pub(crate) struct Record {
    pub(crate) owner: RecordOwner,
    pub(crate) key: &'static str,
    pub(crate) payload: Vec<u8>,
    pub(crate) references: Vec<i64>,
}

#[derive(Default)]
pub(crate) struct Batch {
    pub(crate) upserts: Vec<Record>,
    pub(crate) deletes: Vec<(RecordOwner, &'static str)>,
}

pub(crate) struct SaveBundle {
    pub(crate) expected_generation: u64,
    pub(crate) manifest: Manifest,
    pub(crate) mode: WriteMode,
    pub(crate) added_entities: Vec<i64>,
    pub(crate) removed_entities: Vec<i64>,
    pub(crate) entity_count: usize,
    pub(crate) batch: Batch,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteMode {
    Incremental,
    ReplaceRecords,
    ReplaceAll,
}

pub(crate) struct StoredSave {
    pub(crate) generation: u64,
    pub(crate) entities: Vec<i64>,
    pub(crate) records: Records,
    pub(crate) manifest: Manifest,
}

trait Backend: Send + Sync {
    fn commit(&self, handle: SaveHandle, bundle: SaveBundle) -> Result<Pending<SaveStats>, Error>;
    fn read(&self, handle: SaveHandle) -> Result<Pending<StoredSave>, Error>;
    fn list(&self) -> Result<Pending<Vec<SaveInfo>>, Error>;
}

pub(crate) struct Pending<T>(Mutex<Receiver<Result<T, Error>>>);

impl<T> Pending<T> {
    fn channel() -> (Sender<Result<T, Error>>, Self) {
        let (sender, receiver) = mpsc::channel();
        (sender, Self(Mutex::new(receiver)))
    }

    pub(crate) fn poll(&mut self) -> Option<Result<T, Error>> {
        match self.0.get_mut().expect("receiver poisoned").try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(RecvError.into())),
        }
    }
}

impl From<RecvError> for Error {
    fn from(error: RecvError) -> Self {
        Self::Io(Box::new(error))
    }
}

#[derive(Resource)]
pub(crate) struct Storage(Box<dyn Backend>);

impl Storage {
    pub(crate) fn commit(
        &self,
        handle: SaveHandle,
        bundle: SaveBundle,
    ) -> Result<Pending<SaveStats>, Error> {
        self.0.commit(handle, bundle)
    }

    pub(crate) fn read(&self, handle: SaveHandle) -> Result<Pending<StoredSave>, Error> {
        self.0.read(handle)
    }

    pub(crate) fn list(&self) -> Result<Pending<Vec<SaveInfo>>, Error> {
        self.0.list()
    }
}
