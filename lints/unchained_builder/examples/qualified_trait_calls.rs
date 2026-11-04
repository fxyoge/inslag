#![expect(dead_code)]
trait First {
    fn option(&mut self) -> &mut Self {
        self
    }
}
trait Second {
    fn option(&mut self) -> &mut Self {
        self
    }
}
struct Settings;
impl First for Settings {}
impl Second for Settings {}

fn configure(settings: &mut Settings) {
    First::option(settings);
    Second::option(settings);
}
fn main() {}
