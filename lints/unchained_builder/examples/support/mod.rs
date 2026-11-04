#[derive(Default)]
pub struct Settings(pub usize);

impl Settings {
    pub fn option(&mut self, value: usize) -> &mut Self {
        self.0 = value;
        self
    }

    pub fn callback(&mut self, _: impl FnOnce() -> usize) -> &mut Self {
        self
    }

    pub fn value(&self) -> usize {
        self.0
    }

    pub fn finish(&mut self) {}
}

pub trait Configure {
    fn enabled(&mut self) -> &mut Self;
}

impl Configure for Settings {
    fn enabled(&mut self) -> &mut Self {
        self
    }
}

#[derive(Default)]
pub struct Request {
    value: usize,
}

impl Request {
    pub fn header(mut self, value: usize) -> Self {
        self.value = value;
        self
    }

    pub fn send(self) {}
}
