#![expect(dead_code)]
struct Settings;
impl Settings {
    fn select(&mut self, other: &'static mut Self) -> &mut Self {
        other
    }
}
fn configure(settings: &mut Settings, first: &'static mut Settings, second: &'static mut Settings) {
    settings.select(first);
    settings.select(second);
}
fn main() {}
