#![expect(dead_code)]
#![deny(unfulfilled_lint_expectations)]
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/support/model.rs"
));

#[cfg_attr(
    dylint_lib = "bevy_unnecessary_query_read",
    allow(bevy_unnecessary_query_read)
)]
fn allowed(query: Query<&Health>) -> bool {
    query.is_empty()
}

#[cfg_attr(
    dylint_lib = "bevy_unnecessary_query_read",
    expect(bevy_unnecessary_query_read)
)]
fn expected(query: Query<&Health>) -> bool {
    query.is_empty()
}

fn main() {}
