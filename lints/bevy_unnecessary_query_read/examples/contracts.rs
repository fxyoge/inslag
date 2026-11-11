#![expect(dead_code)]
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/support/model.rs"
));

pub fn public(query: Query<&Health>) -> bool {
    query.is_empty()
}

trait Inspect {
    fn inspect(query: Query<&Health>) -> bool;
}

struct Inspector;
impl Inspect for Inspector {
    fn inspect(query: Query<&Health>) -> bool {
        query.is_empty()
    }
}

impl Inspector {
    fn inherent(query: Query<&Health>) -> bool {
        query.is_empty()
    }
}

fn fixed(query: Query<&Health>) -> bool {
    query.is_empty()
}

fn called(query: Query<&Health>) -> bool {
    query.is_empty()
}

fn caller(query: Query<&Health>) -> bool {
    called(query)
}

fn concrete_ufcs(query: Query<&Health>) -> bool {
    Query::<&Health>::is_empty(&query)
}

fn main() {
    let _fixed: fn(Query<&Health>) -> bool = fixed;
    let _closure: fn(Query<&Health>) -> bool = |query: Query<&Health>| query.is_empty();
    let closure = |query: Query<&Health>| query.is_empty();
    let _fixed_closure: fn(Query<&Health>) -> bool = closure;
}
