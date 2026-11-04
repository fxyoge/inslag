#![expect(dead_code)]
struct Settings;
impl Settings {
    fn select<'a>(mut self: &'a mut Self, other: &'static mut Self) -> &'a mut Self {
        let mut other: &'a mut Self = other;
        std::mem::swap(&mut self, &mut other);
        self
    }
}
fn configure(settings: &mut Settings, first: &'static mut Settings, second: &'static mut Settings) {
    settings.select(first);
    settings.select(second);
}
fn main() {}
