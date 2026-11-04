#![expect(dead_code)]
struct Settings;
unsafe extern "Rust" {
    fn configure_external(settings: &mut Settings) -> &mut Settings;
}
impl Settings {
    fn enabled(&mut self) -> &mut Self {
        unsafe { configure_external(self) }
    }
}
fn configure(settings: &mut Settings) {
    settings.enabled();
    settings.enabled();
}
fn main() {}
