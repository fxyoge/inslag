#![expect(dead_code)]
struct Settings;
impl Settings {
    fn enabled(&mut self, depth: u8) -> &mut Self {
        if depth == 0 {
            self
        } else {
            self.enabled(depth - 1)
        }
    }
}
fn configure(settings: &mut Settings) {
    settings.enabled(1);
    settings.enabled(2);
}
fn main() {}
