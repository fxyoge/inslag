#![expect(dead_code)]
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/support/model.rs"
));

fn get(query: Query<&Health>, entity: Entity) -> bool {
    query.contains(entity) && query.get(entity).is_ok_and(|health| health.0 > 0)
}

fn discarded_get(query: Query<&Health>, entity: Entity) -> bool {
    query.get(entity).is_ok()
}

fn single(query: Query<&Health>) -> bool {
    !query.is_empty() && query.single().is_ok()
}

fn iteration(query: Query<&Health>) -> bool {
    query.is_empty() || query.iter().any(|health| health.0 > 0)
}

fn branch(query: Query<&Health>, read: bool) -> bool {
    if read {
        query.iter().count() > 0
    } else {
        query.is_empty()
    }
}

fn helper(query: Query<&Health>) -> bool {
    query.is_empty() || consume(&query)
}

fn consume(query: &Query<&Health>) -> bool {
    query.iter().any(|health| health.0 > 0)
}

fn returns<'w, 's>(query: Query<'w, 's, &'static Health>) -> Query<'w, 's, &'static Health> {
    let _empty = query.is_empty();
    query
}

fn capture(query: Query<&Health>) -> bool {
    let read = || query.is_empty();
    read()
}

fn nested_capture(query: Query<&Health>) -> bool {
    let read = || || query.is_empty();
    read()()
}

fn unused(_query: Query<&Health>) {}

fn main() {}
