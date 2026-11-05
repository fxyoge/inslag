#![allow(dead_code)]

#[test]
fn test_can_construct_raw_audio() {
    let _ = bevy_audio::AudioPlayer::new(bevy_asset::Handle::default());
}

#[cfg(test)]
mod tests {
    fn helper(_: bevy_audio::AudioSource) {}
}

fn production(_: bevy_audio::AudioPlayer) {}

fn main() {}
