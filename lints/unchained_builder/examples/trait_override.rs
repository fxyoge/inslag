#![expect(dead_code)]
trait Configure {
    fn enabled(&mut self) -> &mut Self {
        self
    }
}
struct Settings {
    child: Option<Box<Self>>,
}
impl Configure for Settings {
    fn enabled(&mut self) -> &mut Self {
        self.child.as_deref_mut().unwrap()
    }
}
fn configure(settings: &mut Settings) {
    settings.enabled();
    settings.enabled();
}
fn main() {}
