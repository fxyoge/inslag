#![expect(dead_code)]
use bevy_input::gamepad::GamepadConnectionEvent;
use bevy_input::gamepad::{GamepadAxis, GamepadButton};
use bevy_input::keyboard::KeyCode;
use bevy_input::mouse::MouseButton;
struct Gamepad;
struct ButtonInput;
mod model {
    pub struct MouseMotion;
    pub struct Gamepad;
}
fn bindings(_: KeyCode, _: MouseButton, _: GamepadAxis, _: GamepadButton) {}
fn disconnected(event: GamepadConnectionEvent) -> bool {
    event.disconnected()
}
fn local(_: Gamepad, _: ButtonInput, _: model::MouseMotion, _: model::Gamepad) {}
mod shadow {
    struct Gamepad;
    fn local(_: Gamepad) {}
}
impl ButtonInput {
    fn pressed(&self) -> bool {
        true
    }
    fn just_pressed(&self) -> bool {
        false
    }
}
#[cfg_attr(
    dylint_lib = "bevy_enforce_bevy_enhanced_input",
    expect(bevy_enforce_bevy_enhanced_input)
)]
mod injection {
    use bevy_input::ButtonInput;
    fn inject(_: ButtonInput<bevy_input::keyboard::KeyCode>) {}
}
fn main() {}
