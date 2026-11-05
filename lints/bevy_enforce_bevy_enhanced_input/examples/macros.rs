#![expect(dead_code)]
macro_rules! framework {
    () => {
        fn generated(_: bevy_input::gamepad::Gamepad) {}
    };
}
macro_rules! user_type {
    ($ty:ty) => {
        fn caller_written(_: $ty) {}
    };
}
framework!();
user_type!(bevy_input::gamepad::Gamepad);
fn main() {}
