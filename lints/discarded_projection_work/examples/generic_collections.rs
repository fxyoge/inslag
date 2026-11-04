#![expect(dead_code)]
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, LinkedList, VecDeque};

struct Report {
    count: usize,
    text: String,
    queue: VecDeque<i32>,
    list: LinkedList<i32>,
    map: BTreeMap<i32, i32>,
    set: BTreeSet<i32>,
    heap: BinaryHeap<i32>,
}

fn project(values: &[i32], text: &String) -> Report {
    Report {
        count: values.len(),
        text: text.clone(),
        queue: values.iter().copied().collect(),
        list: values.iter().copied().collect(),
        map: values.iter().map(|&value| (value, value)).collect(),
        set: values.iter().copied().collect(),
        heap: values.iter().copied().collect(),
    }
}

fn inspect(values: &[i32], text: &String) -> usize {
    project(values, text).count
}
fn main() {}
