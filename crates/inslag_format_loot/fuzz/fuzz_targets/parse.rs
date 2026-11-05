#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &str| {
    if let Err(error) = inslag_format_loot::parse(input) {
        assert!(input.get(error.span).is_some());
    }
});
