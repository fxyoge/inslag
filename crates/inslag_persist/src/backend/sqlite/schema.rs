use crate::{
    Error,
    backend::migrations::{Migration, pending},
};
use rusqlite::{Connection, Transaction};

pub(super) const APPLICATION_ID: i64 = 0x494E_5350;

pub(super) const INITIAL: Migration<&str> = Migration {
    revision: 1,
    name: "0001_initial.up.sql",
    step: include_str!("../../../sqlite/migrations/0001_initial.up.sql"),
};
pub(super) const CURRENT: Schema = Schema(&[INITIAL]);
const UNCLAIMED: &str = include_str!("../../../sqlite/unclaimed.sql");

#[derive(Clone, Copy)]
pub(super) struct Schema(pub(super) &'static [Migration<&'static str>]);

impl Schema {
    pub(super) fn needs_upgrade(
        self,
        connection: &Connection,
        create: bool,
    ) -> Result<bool, Error> {
        Ok(!pending(revision(connection, create)?, self.0)?.is_empty())
    }

    pub(super) fn upgrade(self, transaction: &Transaction, create: bool) -> Result<(), Error> {
        let current = revision(transaction, create)?;
        let steps = pending(current, self.0)?;
        for migration in steps {
            // The operation owns the transaction; migration files must not commit it.
            transaction
                .execute_batch(migration.step)
                .map_err(|source| Error::Migration {
                    revision: migration.revision,
                    name: migration.name,
                    source: Box::new(source),
                })?;
        }
        if let Some(last) = steps.last() {
            if current == 0 {
                transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
            }
            transaction.pragma_update(None, "user_version", last.revision)?;
        }
        Ok(())
    }
}

fn revision(connection: &Connection, create: bool) -> Result<u32, Error> {
    let application = application_id(connection)?;
    let revision: i32 = connection.pragma_query_one(None, "user_version")?;
    match (application, revision) {
        (APPLICATION_ID, 1..) => Ok(revision as u32),
        (0, 0) if create && connection.query_row(UNCLAIMED, [], |row| row.get(0))? => Ok(0),
        _ => Err(Error::Format),
    }
}

pub(super) fn application_id(connection: &Connection) -> Result<i64, Error> {
    Ok(connection.pragma_query_one(None, "application_id")?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::TransactionBehavior;
    use std::{fs, path::Path};

    #[test]
    fn runtime_history_matches_sqlc_migration_directory() {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("sqlite/migrations");
        let mut files: Vec<_> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort_unstable();
        assert_eq!(files.len(), CURRENT.0.len());
        for (file, migration) in files.iter().zip(CURRENT.0) {
            assert_eq!(file, migration.name);
            assert!(file.starts_with(&format!("{:04}_", migration.revision)));
            assert!(file.ends_with(".up.sql"));
            assert_eq!(
                fs::read_to_string(directory.join(file)).unwrap(),
                migration.step
            );
        }
    }

    #[test]
    fn shipped_migrations_do_not_commit_the_callers_transaction() {
        let mut database = Connection::open_in_memory().unwrap();
        let transaction = database
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        CURRENT.upgrade(&transaction, true).unwrap();
        assert!(!CURRENT.needs_upgrade(&transaction, false).unwrap());
        transaction.rollback().unwrap();
        assert!(!database.table_exists(None, "meta").unwrap());
        assert_eq!(revision(&database, true).unwrap(), 0);
    }
}
