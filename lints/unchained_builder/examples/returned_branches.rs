#![expect(dead_code)]
struct Settings(bool);
impl Settings {
    fn enabled(&mut self, condition: bool) -> &mut Self {
        if condition {
            self.0 = true;
            return self;
        }
        self.0 = false;
        self
    }
}
fn configure(settings: &mut Settings) {
    settings.enabled(true);
    settings.enabled(false);
}
fn main() {}
