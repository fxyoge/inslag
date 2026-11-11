#![expect(dead_code)]
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/support/model.rs"
));

fn with_filter(query: Query<&Health, With<Shield>>) -> bool {
    query.is_empty()
}

fn without_filter(query: Query<&Health, Without<Hidden>>) -> bool {
    query.is_empty()
}

fn or_filter(query: Query<&Health, Or<(With<Shield>, Without<Hidden>)>>) -> bool {
    query.is_empty()
}

fn other_change(query: Query<&Health, Changed<Shield>>) -> bool {
    query.is_empty()
}

fn partial_change(query: Query<(&Health, &Shield), Changed<Health>>) -> bool {
    query.is_empty()
}

fn changed(query: Query<&Health, Changed<Health>>) -> bool {
    query.is_empty()
}

fn added(query: Query<&Health, Added<Health>>) -> bool {
    query.is_empty()
}

fn nested_change(query: Query<&Health, Or<(With<Shield>, Changed<Health>)>>) -> bool {
    query.is_empty()
}

fn tuple_changes(query: Query<(&Health, &Shield), (Added<Health>, Changed<Shield>)>) -> bool {
    query.is_empty()
}

fn main() {}
