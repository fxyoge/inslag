#![expect(dead_code)]
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/support/model.rs"
));

fn contains(query: Query<&Health>, entity: Entity) -> bool {
    query.contains(entity)
}

fn empty(query: Query<&Health>) -> bool {
    query.is_empty()
}

fn both(query: Query<&Health>, entity: Entity) -> bool {
    !query.is_empty() && query.contains(entity)
}

fn tuple(query: Query<(Entity, &Health, (&Shield, ()))>, entity: Entity) -> bool {
    query.contains(entity)
}

fn duplicate(query: Query<(&Health, &Health)>) -> bool {
    query.is_empty()
}

fn ufcs(query: Query<&Health>, entity: Entity) -> bool {
    Query::contains(&query, entity) || Query::is_empty(&query)
}

fn renamed(query: bevy_ecs::system::Query<&Health>) -> bool {
    query.is_empty()
}

fn main() {}
