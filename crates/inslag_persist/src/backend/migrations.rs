use crate::Error;

pub(super) struct Migration<T> {
    pub(super) revision: u32,
    pub(super) name: &'static str,
    pub(super) step: T,
}

pub(super) fn pending<T>(current: u32, history: &[Migration<T>]) -> Result<&[Migration<T>], Error> {
    for (expected, migration) in (1..).zip(history) {
        if migration.revision != expected {
            return Err(Error::InvalidData(format!(
                "migration {} has revision {}, expected {expected}",
                migration.name, migration.revision
            )));
        }
    }
    history.get(current as usize..).ok_or(Error::SchemaTooNew {
        found: current,
        supported: history.last().map_or(0, |migration| migration.revision),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(revision: u32) -> Migration<()> {
        Migration {
            revision,
            name: "test",
            step: (),
        }
    }

    #[test]
    fn histories_advance_independently() {
        let first = [step(1), step(2), step(3)];
        let second = [step(1)];
        assert_eq!(pending(0, &first).unwrap().len(), 3);
        assert_eq!(pending(1, &first).unwrap()[0].revision, 2);
        assert!(pending(1, &second).unwrap().is_empty());
        assert!(matches!(
            pending(2, &second),
            Err(Error::SchemaTooNew {
                found: 2,
                supported: 1
            })
        ));
    }

    #[test]
    fn histories_reject_gaps_duplicates_and_out_of_order_steps() {
        for history in [[step(1), step(3)], [step(1), step(1)], [step(2), step(1)]] {
            assert!(matches!(pending(0, &history), Err(Error::InvalidData(_))));
        }
    }
}
