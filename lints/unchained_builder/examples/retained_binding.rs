#![expect(dead_code)]
#[derive(Clone, Copy)]
struct Settings;
impl Settings {
    fn enabled(self) -> Self {
        self
    }
}
fn configure(settings: Settings) {
    let first = settings.enabled();
    let second = first.enabled();
    std::hint::black_box((first, second));
}
fn main() {}
