#![allow(dead_code, unused_imports)]

use bevy_audio::AudioSource as Clip;

type Cached = bevy_asset::Handle<Clip>;

fn raw(_: bevy_audio::AudioPlayer, _: &mut bevy_audio::AudioSink) {}

fn inferred(
    server: &bevy_asset::AssetServer,
    mut assets: bevy_ecs::prelude::ResMut<bevy_asset::Assets<Clip>>,
) {
    let handle = server.load("audio/confirm.wav");
    assets.remove(handle.id());
}

fn construct(handle: Cached) {
    let _ = bevy_audio::AudioPlayer::new(handle.clone());
    let _ = bevy_audio::AudioPlayer(handle);
}

fn backend(_: &rodio::Player) {}

fn main() {}
