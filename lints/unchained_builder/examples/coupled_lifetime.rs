#![expect(dead_code)]
struct Settings;
impl Settings {
    fn option<'a>(&'a mut self, _: &'a usize) -> &'a mut Self {
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
