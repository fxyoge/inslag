#![expect(dead_code)]
mod directory {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/tests/injection.rs"
    ));
}
mod exact {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/tests.rs"
    ));
}
mod suffix {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/input_tests.rs"
    ));
}
// File exemptions must not leak into this production file.
fn production(_: bevy_input::gamepad::Gamepad) {}
fn main() {}
