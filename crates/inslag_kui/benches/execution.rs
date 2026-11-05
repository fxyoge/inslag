mod support;

use divan::Bencher;
use support::{Backend, Choice, Config, Fixture, Flair, Native, Pipeline, Scene, Scxml, Work};

const SINGLE_THREADED: bool = false;

fn main() {
    divan::main();
}

include!("support/suite.rs");
