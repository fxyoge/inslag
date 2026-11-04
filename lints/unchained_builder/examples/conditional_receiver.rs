#![expect(dead_code)]
struct Settings {
    child: Option<Box<Self>>,
}
impl Settings {
    fn select(&mut self, condition: bool) -> &mut Self {
        if condition {
            self
        } else {
            self.child.as_deref_mut().unwrap()
        }
    }
}
fn configure(settings: &mut Settings) {
    settings.select(true);
    settings.select(false);
}
fn main() {}
