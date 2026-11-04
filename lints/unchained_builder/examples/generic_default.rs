#![expect(dead_code)]
trait Configure {
    fn enabled(&mut self) -> &mut Self {
        self
    }
}
fn configure<T: Configure>(settings: &mut T) {
    settings.enabled();
    settings.enabled();
}
fn main() {}
