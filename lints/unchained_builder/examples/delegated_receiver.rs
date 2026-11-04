#![expect(dead_code)]
struct Settings(bool);
impl Settings {
    fn enabled(&mut self) -> &mut Self {
        self.configure(true)
    }
    fn configure(&mut self, enabled: bool) -> &mut Self {
        self.0 = enabled;
        self
    }
}
fn configure(settings: &mut Settings) {
    settings.enabled();
    settings.enabled();
}
fn main() {}
