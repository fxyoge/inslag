use super::*;
use crate::backend::{Record, migrations::Migration};

const V2: Migration<&str> = Migration {
    revision: 2,
    name: "add_record_flags",
    step: "ALTER TABLE records ADD COLUMN flags INTEGER NOT NULL DEFAULT 0 CHECK(flags IN (0, 1));",
};
const V3: Migration<&str> = Migration {
    revision: 3,
    name: "backfill_flags",
    step: "UPDATE records SET flags = 1;",
};
const BROKEN_V3: Migration<&str> = Migration {
    step: "UPDATE records SET flags = 2;",
    ..V3
};
const BASELINE: schema::Schema = schema::Schema(&[schema::INITIAL]);
const UPGRADED: schema::Schema = schema::Schema(&[schema::INITIAL, V2, V3]);
const BROKEN: schema::Schema = schema::Schema(&[schema::INITIAL, V2, BROKEN_V3]);
const HANDLE: SaveHandle = SaveHandle(1);

fn slot(directory: &Path, schema: schema::Schema) -> Database {
    Database {
        path: directory.join(format!("{:032x}.sqlite", HANDLE.0)),
        connection: None,
        schema,
    }
}

fn write(generation: u64, payload: &[u8]) -> SaveBundle {
    SaveBundle {
        expected_generation: generation,
        manifest: [("test".to_owned(), Role::Global)].into(),
        mode: WriteMode::Incremental,
        added_entities: vec![],
        removed_entities: vec![],
        entity_count: 0,
        batch: Batch {
            upserts: vec![Record {
                owner: RecordOwner::GLOBAL,
                key: "test",
                payload: payload.to_owned(),
                references: vec![],
            }],
            deletes: vec![],
        },
    }
}

fn stored_revision(connection: &Connection) -> u32 {
    connection.pragma_query_one(None, "user_version").unwrap()
}

fn flags(connection: &Connection) -> i64 {
    connection
        .query_row("SELECT flags FROM records", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn listing_does_not_upgrade_but_read_applies_pending_steps() {
    let directory = tempfile::tempdir().unwrap();
    let mut old = slot(directory.path(), BASELINE);
    old.commit(write(0, b"before")).unwrap();
    let inspect = Connection::open(&old.path).unwrap();
    assert_eq!(stored_revision(&inspect), 1);
    let listed = list(directory.path()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].handle, HANDLE);
    assert_eq!(stored_revision(&inspect), 1);

    let mut new = slot(directory.path(), UPGRADED);
    let save = new.read().unwrap();
    assert_eq!(save.generation, 1);
    assert_eq!(save.records["test"][0].1, b"before");
    assert_eq!(stored_revision(&inspect), 3);
    assert_eq!(flags(&inspect), 1);
    assert_eq!(list(directory.path()).unwrap(), listed);
    assert_eq!(new.read().unwrap().generation, 1);

    // Even with a cached connection, the old binary must not write the newer layout.
    assert!(matches!(
        old.commit(write(1, b"old writer")),
        Err(Error::SchemaTooNew {
            found: 3,
            supported: 1
        })
    ));
    assert_eq!(new.read().unwrap().records["test"][0].1, b"before");
}

#[test]
fn failed_upgrade_rolls_back_every_step_and_can_be_retried() {
    let directory = tempfile::tempdir().unwrap();
    let mut old = slot(directory.path(), BASELINE);
    old.commit(write(0, b"before")).unwrap();
    let inspect = Connection::open(&old.path).unwrap();
    let mut broken = slot(directory.path(), BROKEN);
    assert!(matches!(
        broken.read(),
        Err(Error::Migration {
            revision: 3,
            name: "backfill_flags",
            ..
        })
    ));
    assert_eq!(stored_revision(&inspect), 1);
    assert!(!inspect.column_exists(None, "records", "flags").unwrap());
    assert_eq!(old.read().unwrap().records["test"][0].1, b"before");

    let mut repaired = slot(directory.path(), UPGRADED);
    assert_eq!(repaired.read().unwrap().generation, 1);
    assert_eq!(stored_revision(&inspect), 3);
    assert_eq!(flags(&inspect), 1);
}

#[test]
fn upgrade_and_commit_share_a_transaction() {
    let directory = tempfile::tempdir().unwrap();
    let mut old = slot(directory.path(), BASELINE);
    old.commit(write(0, b"before")).unwrap();
    let inspect = Connection::open(&old.path).unwrap();
    let mut new = slot(directory.path(), UPGRADED);
    assert!(matches!(
        new.commit(write(2, b"conflict")),
        Err(Error::Conflict {
            expected: 2,
            actual: 1
        })
    ));
    assert_eq!(stored_revision(&inspect), 1);
    assert!(!inspect.column_exists(None, "records", "flags").unwrap());
    assert_eq!(new.commit(write(1, b"after")).unwrap().generation, 2);
    assert_eq!(stored_revision(&inspect), 3);
    assert_eq!(new.read().unwrap().records["test"][0].1, b"after");
}

#[test]
fn legacy_file_is_not_listed_or_modified() {
    let directory = tempfile::tempdir().unwrap();
    let mut database = slot(directory.path(), schema::CURRENT);
    let legacy = Connection::open(&database.path).unwrap();
    let legacy_id = 0x4749_4741_i64;
    legacy
        .pragma_update(None, "application_id", legacy_id)
        .unwrap();
    legacy.pragma_update(None, "user_version", 2).unwrap();
    let original = fs::read(&database.path).unwrap();

    assert!(list(directory.path()).unwrap().is_empty());
    assert!(matches!(database.read(), Err(Error::Format)));
    assert!(matches!(
        database.commit(write(0, b"new")),
        Err(Error::Format)
    ));
    assert_eq!(fs::read(&database.path).unwrap(), original);
}
