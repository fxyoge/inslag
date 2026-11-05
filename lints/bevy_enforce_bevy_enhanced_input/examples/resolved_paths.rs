#![expect(dead_code)]
use bevy_input::ButtonInput as Buttons;
use bevy_input::keyboard::KeyCode;
type Input = Buttons<KeyCode>;
type Nested = Option<Input>;
fn alias(_: Nested) {}
fn qualified(_: bevy_input::gamepad::Gamepad) {}
mod facade {
    pub use bevy_input::mouse::MouseMotion as Motion;
}
fn reexport(_: facade::Motion) {}
mod prelude {
    use bevy_input::prelude::*;
    fn glob(_: Gamepad) {}
}
fn expression() {
    let _ = Buttons::<KeyCode>::default();
}
fn main() {}
