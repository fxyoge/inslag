#![expect(dead_code)]
struct Settings;
impl Settings {
    fn select(mut self: &mut Self, other: &'static mut Self) -> &mut Self {
        self = other;
        self
    }
}
fn configure(settings: &mut Settings, first: &'static mut Settings, second: &'static mut Settings) {
    settings.select(first);
    settings.select(second);
}
fn main() {}
