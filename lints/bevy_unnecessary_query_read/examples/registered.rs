include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/support/model.rs"
));

fn condition(query: Query<&Health>) -> bool {
    !query.is_empty()
}

fn system(query: Query<&Shield>) {
    let _empty = query.is_empty();
}

fn main() {
    Schedule::default().add_systems(system.run_if(condition));
    Schedule::default().add_systems(|query: Query<&Health>| {
        let _empty = query.is_empty();
    });
    let _closure = |query: Query<&Health>| query.is_empty();
}
