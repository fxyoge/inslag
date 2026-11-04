#![expect(dead_code)]
use std::cmp::Ordering;
use std::collections::BTreeSet;
#[derive(PartialEq, Eq)]
struct Key(u64);
impl Ord for Key {
    fn cmp(&self, other: &Self) -> Ordering {
        println!("comparing keys");
        self.0.cmp(&other.0)
    }
}
impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
struct Report {
    count: usize,
    keys: BTreeSet<Key>,
}
fn project(values: &[u64]) -> Report {
    Report {
        count: values.len(),
        keys: values.iter().map(|&value| Key(value)).collect(),
    }
}
fn inspect(values: &[u64]) -> usize {
    project(values).count
}
fn main() {}
