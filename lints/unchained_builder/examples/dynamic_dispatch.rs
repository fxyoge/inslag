#![expect(dead_code)]
trait Configure {
    fn enabled(&mut self) -> &mut dyn Configure;
}
fn configure(settings: &mut dyn Configure) {
    settings.enabled();
    settings.enabled();
}
fn main() {}
