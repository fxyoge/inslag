#![expect(dead_code)]
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/support/model.rs"
));

fn moved(query: Query<&Health>) -> bool {
    let alias = query;
    alias.is_empty()
}

fn borrowed(query: Query<&Health>) -> bool {
    let alias = &query;
    let second = &alias;
    second.is_empty()
}

fn parenthesized(query: Query<&Health>) -> bool {
    (&query).is_empty()
}

fn annotated(query: Query<&Health>) -> bool {
    let alias: Query<&Health> = query;
    alias.is_empty()
}

#[expect(unused_assignments)]
fn reassigned<'w, 's>(
    mut query: Query<'w, 's, &'static Health>,
    other: Query<'w, 's, &'static Health>,
) -> bool {
    query = other;
    query.is_empty()
}

#[expect(unused_assignments)]
fn alias_reassigned<'w, 's>(
    query: Query<'w, 's, &'static Health>,
    other: Query<'w, 's, &'static Health>,
) -> bool {
    let mut alias = query;
    alias = other;
    alias.is_empty()
}

fn alias_read(query: Query<&Health>, entity: Entity) -> bool {
    let alias = query;
    alias.contains(entity) && alias.get(entity).is_ok_and(|health| health.0 > 0)
}

fn main() {}
