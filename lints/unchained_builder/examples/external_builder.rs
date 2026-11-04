#![expect(dead_code)]
use std::process::Command;

fn configure(command: &mut Command) {
    command.arg("first");
    command.arg("second");
}
fn main() {}
