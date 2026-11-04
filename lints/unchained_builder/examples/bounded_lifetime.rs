#![expect(dead_code)]
struct Settings;
impl Settings {
    fn option<'a, T: 'a>(&'a mut self, _: T) -> &'a mut Self {
        self
    }
}
fn configure(settings: &mut Settings, mut value: usize) {
    settings.option(&value);
    settings.option({
        value += 1;
        &value
    });
}
fn main() {}
