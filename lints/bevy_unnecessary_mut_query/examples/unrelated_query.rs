#![expect(dead_code)]

struct Query<T>(T);

fn update(query: Query<&mut u32>) {
    let _ = *query.0;
}

fn main() {}
