use super::{
    Backend, Batch, Error, Manifest, Pending, RecordOwner, Records, Role, SaveBundle, SaveHandle,
    SaveInfo, SaveStats, StoredSave, WriteMode,
};
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{Mutex, mpsc},
    thread,
    time::{Duration, UNIX_EPOCH},
};

// Unused helpers and broader visibility are emitted by the plugin.
#[allow(dead_code, unreachable_pub, unused_variables, clippy::all)]
mod queries;
mod schema;
#[cfg(test)]
mod tests;

use queries::*;

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Io(Box::new(error))
    }
}

pub(super) struct Sqlite {
    directory: PathBuf,
    worker: Mutex<Option<mpsc::Sender<Job>>>,
}

type Job = Box<dyn FnOnce(&mut Collection) + Send>;

struct Collection {
    directory: PathBuf,
    database: Option<(SaveHandle, Database)>,
}

struct Database {
    path: PathBuf,
    connection: Option<Connection>,
    schema: schema::Schema,
}

impl Sqlite {
    pub(super) fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            worker: Mutex::default(),
        }
    }

    fn submit<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Collection) -> Result<T, Error> + Send + 'static,
    ) -> Result<Pending<T>, Error> {
        let mut worker = self.worker.lock().expect("worker poisoned");
        if worker.is_none() {
            let (send, receive) = mpsc::channel::<Job>();
            let directory = self.directory.clone();
            thread::Builder::new()
                .name("inslag-sqlite".into())
                .spawn(move || {
                    // Closing the last connection can itself write to SQLite's WAL.
                    let mut collection = Collection {
                        directory,
                        database: None,
                    };
                    for job in receive {
                        job(&mut collection);
                    }
                })?;
            *worker = Some(send);
        }
        let (send, pending) = Pending::channel();
        worker
            .as_ref()
            .expect("initialized worker")
            .send(Box::new(move |collection| {
                let _ = send.send(operation(collection));
            }))
            .map_err(|_| mpsc::RecvError)?;
        Ok(pending)
    }
}

impl Backend for Sqlite {
    fn commit(&self, handle: SaveHandle, bundle: SaveBundle) -> Result<Pending<SaveStats>, Error> {
        self.submit(move |collection| {
            if bundle.expected_generation == 0 {
                fs::create_dir_all(&collection.directory)?;
            }
            collection.database(handle).commit(bundle)
        })
    }

    fn read(&self, handle: SaveHandle) -> Result<Pending<StoredSave>, Error> {
        self.submit(move |collection| collection.database(handle).read())
    }

    fn list(&self) -> Result<Pending<Vec<SaveInfo>>, Error> {
        self.submit(|collection| list(&collection.directory))
    }
}

impl Collection {
    fn database(&mut self, handle: SaveHandle) -> &mut Database {
        if self
            .database
            .as_ref()
            .is_none_or(|(active, _)| *active != handle)
        {
            self.database = Some((
                handle,
                Database {
                    path: self.directory.join(format!("{:032x}.sqlite", handle.0)),
                    connection: None,
                    schema: schema::CURRENT,
                },
            ));
        }
        &mut self.database.as_mut().expect("selected database").1
    }
}

fn list(directory: &Path) -> Result<Vec<SaveInfo>, Error> {
    let files = match fs::read_dir(directory) {
        Ok(files) => files,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut saves = Vec::new();
    for entry in files {
        let entry = entry?;
        let name = entry.file_name();
        let Some(stem) = name.to_str().and_then(|name| name.strip_suffix(".sqlite")) else {
            continue;
        };
        if stem.len() != 32
            || !stem
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || !entry.file_type()?.is_file()
        {
            continue;
        }
        let mut connection =
            Connection::open_with_flags(entry.path(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let transaction = connection.transaction()?;
        if schema::application_id(&transaction)? != schema::APPLICATION_ID {
            continue;
        }
        // The slot header is kept readable across revisions so discovery never upgrades a file.
        let header = read_header(&transaction)?;
        if header.generation != 0 {
            let millis = u64::try_from(header.created_at_ms)
                .map_err(|_| Error::InvalidData("negative creation timestamp".into()))?;
            let created_at = UNIX_EPOCH
                .checked_add(Duration::from_millis(millis))
                .ok_or_else(|| Error::InvalidData("creation timestamp out of range".into()))?;
            saves.push(SaveInfo {
                handle: SaveHandle(u128::from_str_radix(stem, 16).expect("validated handle")),
                created_at,
            });
        }
    }
    saves.sort_unstable_by_key(|save| save.handle);
    Ok(saves)
}

impl Database {
    fn connection(&mut self, create: bool) -> Result<&mut Connection, Error> {
        if self.connection.is_none() {
            let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE;
            if create {
                flags |= OpenFlags::SQLITE_OPEN_CREATE;
            }
            let mut connection = Connection::open_with_flags(&self.path, flags)?;
            connection.busy_timeout(Duration::from_secs(5))?;
            let transaction = connection.transaction()?;
            self.schema.needs_upgrade(&transaction, create)?;
            transaction.commit()?;
            let journal: String =
                connection
                    .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
            if journal != "wal" {
                return Err(Error::InvalidData("WAL mode unavailable".into()));
            }
            connection.pragma_update(None, "synchronous", "FULL")?;
            connection.pragma_update(None, "foreign_keys", true)?;
            self.connection = Some(connection);
        }
        Ok(self.connection.as_mut().expect("initialized connection"))
    }

    fn commit(&mut self, mut bundle: SaveBundle) -> Result<SaveStats, Error> {
        // Dirty sets have hash order. Cluster writes to avoid bouncing between pages.
        bundle
            .batch
            .upserts
            .sort_unstable_by_key(|record| (record.owner, record.key));
        let create = bundle.expected_generation == 0;
        let schema = self.schema;
        let transaction = self
            .connection(create)?
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        schema.upgrade(&transaction, create)?;
        let previous = read_header(&transaction)?.generation;
        let expected = bundle.expected_generation;
        if expected != previous as u64 {
            return Err(Error::Conflict {
                expected,
                actual: previous as u64,
            });
        }
        let next = previous
            .checked_add(1)
            .ok_or_else(|| Error::InvalidData("generation exhausted".into()))?;
        prepare_write(&transaction, &bundle, previous == 0)?;
        write_batch(
            &transaction,
            &bundle.added_entities,
            &bundle.removed_entities,
            &bundle.batch,
        )?;
        AdvanceGeneration::builder()
            .next_generation(next)
            .build()
            .execute(&transaction)?;
        transaction.commit()?;
        Ok(SaveStats {
            generation: next as u64,
            entities: bundle.entity_count,
            upserts: bundle.batch.upserts.len(),
            bytes: bundle
                .batch
                .upserts
                .iter()
                .map(|record| record.payload.len())
                .sum(),
        })
    }

    fn read(&mut self) -> Result<StoredSave, Error> {
        match fs::metadata(&self.path) {
            Err(error) if error.kind() == ErrorKind::NotFound => return Err(Error::NotFound),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let schema = self.schema;
        let connection = self.connection(false)?;
        let transaction = connection.transaction()?;
        let transaction = if schema.needs_upgrade(&transaction, false)? {
            // A read snapshot cannot be promoted safely if another writer has advanced it.
            transaction.rollback()?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            schema.upgrade(&transaction, false)?;
            transaction
        } else {
            transaction
        };
        let save = read_save(&transaction)?;
        transaction.commit()?;
        Ok(save)
    }
}

fn prepare_write(
    transaction: &Transaction,
    bundle: &SaveBundle,
    first_commit: bool,
) -> Result<(), Error> {
    let rewrite = bundle.mode == WriteMode::ReplaceAll;
    if rewrite {
        clear_records(transaction)?;
        ClearManifest.execute(transaction)?;
    }
    if first_commit || rewrite {
        let mut insert = transaction.prepare_cached(InsertType::QUERY)?;
        for (key, role) in &bundle.manifest {
            let kind = match role {
                Role::Global => 0,
                Role::Component => 1,
            };
            let command = InsertType::builder().key(key).kind(kind).build();
            insert.execute(command.as_params())?;
        }
    }
    if read_manifest(transaction)? != bundle.manifest {
        return Err(Error::Manifest);
    }
    if bundle.mode == WriteMode::ReplaceRecords {
        clear_records(transaction)?;
    }
    Ok(())
}

fn clear_records(transaction: &Transaction) -> Result<(), Error> {
    ClearRecords.execute(transaction)?;
    ClearEntities.execute(transaction)?;
    Ok(())
}

fn write_batch(
    transaction: &Transaction,
    added_entities: &[i64],
    removed_entities: &[i64],
    batch: &Batch,
) -> Result<(), Error> {
    let mut insert = transaction.prepare_cached(InsertEntity::QUERY)?;
    for &id in added_entities {
        let command = InsertEntity::builder().id(id).build();
        insert.execute(command.as_params())?;
    }
    let mut delete = transaction.prepare_cached(DeleteRecord::QUERY)?;
    for &(owner, key) in &batch.deletes {
        let command = DeleteRecord::builder()
            .entity(owner.entity_id().unwrap_or(0))
            .type_key(key)
            .build();
        delete.execute(command.as_params())?;
    }
    let mut upsert = transaction.prepare_cached(UpsertRecord::QUERY)?;
    let mut clear_refs = transaction.prepare_cached(ClearReferences::QUERY)?;
    let mut insert_ref = transaction.prepare_cached(InsertReference::QUERY)?;
    for record in &batch.upserts {
        let entity = record.owner.entity_id().unwrap_or(0);
        let command = UpsertRecord::builder()
            .entity(entity)
            .type_key(record.key)
            .payload(&record.payload)
            .build();
        upsert.execute(command.as_params())?;
        let command = ClearReferences::builder()
            .source_entity(entity)
            .source_type(record.key)
            .build();
        clear_refs.execute(command.as_params())?;
        for &target in &record.references {
            let command = InsertReference::builder()
                .source_entity(entity)
                .source_type(record.key)
                .target(target)
                .build();
            insert_ref.execute(command.as_params())?;
        }
    }
    let mut delete_entity = transaction.prepare_cached(DeleteEntity::QUERY)?;
    for &id in removed_entities {
        let command = DeleteEntity::builder().id(id).build();
        delete_entity.execute(command.as_params())?;
    }
    Ok(())
}

fn read_save(transaction: &Transaction) -> Result<StoredSave, Error> {
    let header = read_header(transaction)?;
    if header.generation == 0 {
        return Err(Error::NotFound);
    }
    let manifest = read_manifest(transaction)?;
    let entities: Vec<_> = ReadEntities
        .query_many(transaction)?
        .into_iter()
        .map(|row| row.id)
        .collect();
    if entities.first().is_some_and(|id| *id < 0) {
        return Err(Error::InvalidData("negative persistent entity ID".into()));
    }
    let mut records = Records::new();
    let mut statement = ReadRecords.prepare(transaction)?;
    for row in statement.query_map(ReadRecords.as_params(), ReadRecordsRow::from_row)? {
        let row = row?;
        let owner = if row.entity == 0 {
            RecordOwner::GLOBAL
        } else if entities.binary_search(&row.entity).is_ok() {
            RecordOwner::entity(row.entity)
        } else {
            return Err(Error::InvalidData(
                "record references a missing entity".into(),
            ));
        };
        records
            .entry(row.type_key)
            .or_default()
            .push((owner, row.payload));
    }
    Ok(StoredSave {
        generation: header.generation as u64,
        entities,
        records,
        manifest,
    })
}

fn read_header(transaction: &Transaction) -> Result<SlotHeaderRow, Error> {
    let header = SlotHeader.query_one(transaction)?;
    if header.generation < 0 {
        return Err(Error::InvalidData("negative save generation".into()));
    }
    Ok(header)
}

fn read_manifest(transaction: &Transaction) -> Result<Manifest, Error> {
    ReadManifest
        .query_many(transaction)?
        .into_iter()
        .map(|row| {
            let role = match row.kind {
                0 => Role::Global,
                1 => Role::Component,
                _ => return Err(Error::InvalidData("invalid persistent type role".into())),
            };
            Ok((row.key, role))
        })
        .collect()
}
