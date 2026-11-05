#![expect(dead_code)]
#[cfg(test)]
mod tests {
    use bevy_input::ButtonInput;
    fn injection(_: ButtonInput<bevy_input::keyboard::KeyCode>) {}
}
#[test]
fn injection() {
    let input = bevy_input::ButtonInput::<bevy_input::keyboard::KeyCode>::default();
    assert!(!input.pressed(bevy_input::keyboard::KeyCode::Space));
}
// This must be reported even with --test.
fn production(_: bevy_input::gamepad::Gamepad) {}
fn main() {}

#[cfg(test)]
mod external_test_support {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/support/helper.rs"
    ));
}
