#![expect(dead_code)]
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/examples/support/model.rs"
));

macro_rules! check {
    ($query:expr) => {
        $query.is_empty()
    };
}

fn macro_use(query: Query<&Health>) -> bool {
    check!(query)
}

macro_rules! system {
    () => {
        fn generated(query: Query<&Health>) -> bool {
            query.is_empty()
        }
    };
}

system!();
fn main() {}
