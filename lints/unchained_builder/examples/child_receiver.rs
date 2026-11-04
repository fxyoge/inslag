#![expect(dead_code)]
struct Settings {
    child: Option<Box<Self>>,
}
impl Settings {
    fn child(&mut self) -> &mut Self {
        self.child.as_deref_mut().unwrap()
    }
}
fn configure(settings: &mut Settings) {
    settings.child();
    settings.child();
}
fn main() {}
