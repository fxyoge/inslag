#![expect(dead_code)]
trait Configure {
    fn enabled(&mut self) -> &mut Self {
        self
    }
}
struct Settings;
impl Configure for Settings {}

fn configure(settings: &mut Settings) {
    settings.enabled();
    settings.enabled();
}
fn main() {}
