#![expect(dead_code)]
struct Settings(bool);
impl Settings {
    fn enabled(&mut self) -> &mut Self {
        let alias = &mut *self;
        alias.0 = true;
        alias
    }
}
fn configure(settings: &mut Settings) {
    settings.enabled();
    settings.enabled();
}
fn main() {}
