#![allow(dead_code)]

macro_rules! raw_player {
    () => {
        fn generated(_: bevy_audio::AudioPlayer) {}
    };
}
macro_rules! caller_type {
    ($ty:ty) => {
        fn caller(_: $ty) {}
    };
}

raw_player!();
caller_type!(bevy_audio::AudioSource);

fn main() {}
