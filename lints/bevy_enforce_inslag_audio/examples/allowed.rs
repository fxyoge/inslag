#![allow(dead_code)]

use bevy_app::App;
use bevy_asset::{AssetServer, Assets, Handle};
use bevy_audio::{AudioPlugin, SpatialListener, Volume};

struct AudioSource;
struct AudioPlayer;

fn configure(app: &mut App) {
    app.add_plugins(AudioPlugin::default());
}

fn values(_: Volume, _: SpatialListener, _: AudioSource, _: AudioPlayer) {}
fn unrelated(
    _: &AssetServer,
    _: &Assets<bevy_asset::LoadedFolder>,
    _: Handle<bevy_asset::LoadedFolder>,
) {
}

fn main() {}

fn other_assets(server: &AssetServer) {
    let _: Handle<bevy_asset::LoadedFolder> = server.load("ui/folder");
}

enum Shape {
    Circle,
    Square,
}

fn domain_data(shape: Shape) -> &'static str {
    match shape {
        Shape::Circle => "circle.png",
        Shape::Square => "square.png",
    }
}
