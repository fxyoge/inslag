use super::{Backend, Error, Pending, SaveBundle, SaveStats, StoredSave};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Mutex, mpsc},
    thread,
    time::Duration,
};

const APPLICATION_ID: i64 = 0x4749_4741;
const FORMAT: i64 = 1;

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Io(Box::new(error))
    }
}

pub(super) struct Sqlite {
    path: PathBuf,
    worker: Mutex<Option<mpsc::Sender<Job>>>,
}

type Job = Box<dyn FnOnce(&mut Database) + Send>;

struct Database {
    path: PathBuf,
    connection: Option<Connection>,
}

impl Sqlite {
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            path,
            worker: Mutex::default(),
        }
    }

    fn submit<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Database) -> Result<T, Error> + Send + 'static,
    ) -> Result<Pending<T>, Error> {
        let mut worker = self.worker.lock().expect("worker poisoned");
        if worker.is_none() {
            let (send, receive) = mpsc::channel::<Job>();
            let path = self.path.clone();
            thread::Builder::new()
                .name("inslag-sqlite".into())
                .spawn(move || {
                    // Closing the last connection can itself write to SQLite's WAL.
                    let mut database = Database {
                        path,
                        connection: None,
                    };
                    for job in receive {
                        job(&mut database);
                    }
                })?;
            *worker = Some(send);
        }
        let (send, receive) = mpsc::channel();
        worker
            .as_ref()
            .expect("initialized worker")
            .send(Box::new(move |database| {
                let _ = send.send(operation(database));
            }))
            .map_err(|_| mpsc::RecvError)?;
        Ok(Pending(Mutex::new(receive)))
    }
}

impl Backend for Sqlite {
    fn commit(&self, bundle: SaveBundle) -> Result<Pending<SaveStats>, Error> {
        self.submit(move |database| database.commit(bundle))
    }

    fn read(&self) -> Result<Pending<Option<StoredSave>>, Error> {
        self.submit(Database::read)
    }
}

impl Database {
    fn connection(&mut self, create: bool) -> Result<&mut Connection, Error> {
        let slot = &mut self.connection;
        if slot.is_none() {
            let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE;
            if create {
                flags |= OpenFlags::SQLITE_OPEN_CREATE;
            }
            let mut connection = Connection::open_with_flags(&self.path, flags)?;
            let application: i64 =
                connection.query_row("PRAGMA application_id", [], |row| row.get(0))?;
            if application == 0 && create {
                let tables: i64 = connection.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                    [],
                    |row| row.get(0),
                )?;
                if tables != 0 {
                    return Err(Error::Format);
                }
                let transaction =
                    connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
                transaction.execute_batch(
                    "PRAGMA application_id = 1195984705;
                     PRAGMA user_version = 1;
                     CREATE TABLE meta (
                         singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                         generation INTEGER NOT NULL CHECK(generation >= 0)
                     ) STRICT;
                     INSERT INTO meta VALUES (1, 0);
                     CREATE TABLE types (
                         key TEXT PRIMARY KEY, kind INTEGER NOT NULL CHECK(kind IN (0, 1))
                     ) STRICT;
                     CREATE TABLE entities (id INTEGER PRIMARY KEY CHECK(id >= 0)) STRICT;
                     INSERT INTO entities VALUES (0);
                     CREATE TABLE records (
                         entity INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
                         type_key TEXT NOT NULL REFERENCES types(key),
                         payload BLOB NOT NULL,
                         PRIMARY KEY(entity, type_key)
                     ) STRICT;
                     CREATE TABLE entity_refs (
                         source_entity INTEGER NOT NULL,
                         source_type TEXT NOT NULL,
                         target INTEGER NOT NULL REFERENCES entities(id) DEFERRABLE INITIALLY DEFERRED,
                         FOREIGN KEY(source_entity, source_type) REFERENCES records(entity, type_key) ON DELETE CASCADE,
                         PRIMARY KEY(source_entity, source_type, target)
                     ) STRICT;
                     CREATE INDEX entity_refs_target ON entity_refs(target);"
                )?;
                transaction.commit()?;
            } else if application != APPLICATION_ID {
                return Err(Error::Format);
            }
            let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
            if version != FORMAT {
                return Err(Error::Format);
            }
            connection.busy_timeout(Duration::from_secs(5))?;
            let journal: String =
                connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
            if journal != "wal" {
                return Err(Error::InvalidData("WAL mode unavailable".into()));
            }
            connection.execute_batch("PRAGMA synchronous = FULL; PRAGMA foreign_keys = ON;")?;
            *slot = Some(connection);
        }
        Ok(slot.as_mut().expect("initialized connection"))
    }
    fn commit(&mut self, mut bundle: SaveBundle) -> Result<SaveStats, Error> {
        // Dirty sets have hash order. Cluster writes to avoid bouncing between pages.
        bundle
            .batch
            .upserts
            .sort_unstable_by(|a, b| (a.id, a.key).cmp(&(b.id, b.key)));
        let transaction = self
            .connection(true)?
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous = generation(&transaction)?;
        let expected = bundle.expected_generation.unwrap_or(0);
        if expected != previous as u64 {
            if bundle.expected_generation.is_none() {
                return Err(Error::LoadRequired);
            }
            return Err(Error::Conflict {
                expected,
                actual: previous as u64,
            });
        }
        let next = previous
            .checked_add(1)
            .ok_or_else(|| Error::InvalidData("generation exhausted".into()))?;
        let mut deletions = 0;
        if bundle.rewrite_manifest {
            deletions += transaction.execute("DELETE FROM records", [])?;
            transaction.execute("DELETE FROM entities WHERE id != 0", [])?;
            transaction.execute("DELETE FROM types", [])?;
        }
        if previous == 0 || bundle.rewrite_manifest {
            for (key, kind) in &bundle.manifest {
                transaction.execute(
                    "INSERT INTO types(key, kind) VALUES (?1, ?2)",
                    params![key, kind],
                )?;
            }
        }
        if read_manifest(&transaction)? != bundle.manifest {
            return Err(Error::Manifest);
        }
        if bundle.replace_records {
            deletions += transaction.execute("DELETE FROM records", [])?;
            transaction.execute("DELETE FROM entities WHERE id != 0", [])?;
        }
        {
            let mut insert =
                transaction.prepare_cached("INSERT OR IGNORE INTO entities(id) VALUES (?1)")?;
            for id in bundle.added_entities {
                insert.execute([id])?;
            }
            let mut delete = transaction
                .prepare_cached("DELETE FROM records WHERE entity = ?1 AND type_key = ?2")?;
            for (id, key) in bundle.batch.deletes {
                deletions += delete.execute(params![id, key])?;
            }
            let mut upsert = transaction.prepare_cached(
                "INSERT INTO records(entity, type_key, payload) VALUES (?1, ?2, ?3)
                 ON CONFLICT(entity, type_key) DO UPDATE SET payload = excluded.payload",
            )?;
            let mut clear_refs = transaction.prepare_cached(
                "DELETE FROM entity_refs WHERE source_entity = ?1 AND source_type = ?2",
            )?;
            let mut insert_ref =
                transaction.prepare_cached("INSERT INTO entity_refs VALUES (?1, ?2, ?3)")?;
            for record in &bundle.batch.upserts {
                upsert.execute(params![record.id, record.key, record.payload])?;
                clear_refs.execute(params![record.id, record.key])?;
                for target in &record.references {
                    insert_ref.execute(params![record.id, record.key, target])?;
                }
            }
            let mut delete_entity =
                transaction.prepare_cached("DELETE FROM entities WHERE id = ?1")?;
            for id in bundle.removed_entities {
                delete_entity.execute([id])?;
            }
        }
        transaction.execute(
            "UPDATE meta SET generation = ?1 WHERE singleton = 1",
            [next],
        )?;
        transaction.commit()?;
        Ok(SaveStats {
            generation: next as u64,
            entities: bundle.entity_count,
            upserts: bundle.batch.upserts.len(),
            deletions,
            bytes: bundle
                .batch
                .upserts
                .iter()
                .map(|record| record.payload.len())
                .sum(),
        })
    }

    fn read(&mut self) -> Result<Option<StoredSave>, Error> {
        if self.connection.is_none() {
            match std::fs::metadata(&self.path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
                Ok(_) => {}
            }
        }
        let transaction = self.connection(false)?.transaction()?;
        let saved_generation = generation(&transaction)?;
        if saved_generation == 0 {
            return Ok(None);
        }
        let manifest = read_manifest(&transaction)?;
        let entities = transaction
            .prepare("SELECT id FROM entities WHERE id != 0 ORDER BY id")?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut records: BTreeMap<String, Vec<(i64, Vec<u8>)>> = BTreeMap::new();
        {
            let mut statement =
                transaction.prepare("SELECT entity, type_key, payload FROM records")?;
            for row in statement.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })? {
                let (id, key, payload) = row?;
                if id != 0 && entities.binary_search(&id).is_err() {
                    return Err(Error::InvalidData(
                        "record references a missing entity".into(),
                    ));
                }
                records.entry(key).or_default().push((id, payload));
            }
        }
        transaction.commit()?;
        Ok(Some(StoredSave {
            generation: saved_generation as u64,
            entities,
            records,
            manifest,
        }))
    }
}

fn generation(connection: &Connection) -> Result<i64, Error> {
    let generation = connection.query_row(
        "SELECT generation FROM meta WHERE singleton = 1",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if generation < 0 {
        return Err(Error::InvalidData("negative save generation".into()));
    }
    Ok(generation)
}

fn read_manifest(connection: &Connection) -> Result<BTreeMap<String, i64>, Error> {
    Ok(connection
        .prepare("SELECT key, kind FROM types")?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<Result<BTreeMap<_, _>, _>>()?)
}
