#![expect(dead_code)]
struct Settings {
    child: Option<Box<Self>>,
}
impl Settings {
    fn select(&mut self, count: usize) -> &mut Self {
        let mut current = self;
        for _ in 0..count {
            current = current.child.as_deref_mut().unwrap();
        }
        current
    }
}
fn configure(settings: &mut Settings) {
    settings.select(1);
    settings.select(2);
}
fn main() {}
