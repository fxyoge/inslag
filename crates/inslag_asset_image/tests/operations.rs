use inslag_asset_image::*;
use inslag_asset_pipeline::{Asset, Context, Operation, Pipeline, Result};
use serde::Serialize;
use std::fs;

#[derive(Serialize)]
struct Fixture(Frames);
impl Operation for Fixture {
    type Output = Frames;
    fn implementation(&self) -> &'static str {
        "frames"
    }
    fn execute(&self, _: &Context<'_>) -> Result<Frames> {
        Ok(self.0.clone())
    }
}
fn frame(color: [u8; 4], depth: f32, pose: u32) -> Frame {
    Frame {
        image: Image {
            width: 2,
            height: 2,
            rgba: color.repeat(4),
        },
        depth: Some(vec![depth; 4]),
        variants: vec![("pose".into(), pose)],
        time: None,
    }
}
fn output(asset: Asset<Frames>) -> Result<Image> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("Cargo.lock"), "version=4\n")?;
    let mut p = Pipeline::new(root.path())?;
    p.export("image.png", png(asset, 0))?;
    p.generate()?;
    Image::read_png(&fs::read(root.path().join("assets/generated/image.png"))?)
}

#[test]
fn occlusion_matches_variant_coordinates_not_array_positions() -> Result<()> {
    let moving = Asset::new(Fixture(Frames(vec![frame([255; 4], 2.0, 1)])));
    let static_frames = Asset::new(Fixture(Frames(vec![
        frame([255; 4], 3.0, 0),
        frame([255; 4], 1.0, 1),
    ])));
    let resolved = Pixels::new(moving, [1, 1])
        .occluded_by(static_frames)
        .finish();
    assert_eq!(output(resolved)?.rgba, [0, 0, 0, 0]);
    Ok(())
}

#[test]
fn fit_is_nearest_centered_and_does_not_upscale() -> Result<()> {
    let source = Asset::new(Fixture(Frames(vec![frame([220, 100, 40, 255], 1.0, 0)])));
    let image = output(Transform::new(source).fit([4, 4]).grayscale().finish())?;
    assert_eq!((image.width, image.height), (4, 4));
    assert_eq!(&image.rgba[..4], &[0; 4]);
    let center = &image.rgba[(4 + 1) * 4..(4 + 2) * 4];
    assert_eq!(center, &[129, 129, 129, 255]);
    Ok(())
}

#[test]
fn composites_clip_negative_positions_and_preserve_transparent_rgb_contract() -> Result<()> {
    let source = Asset::new(Fixture(Frames(vec![frame([200, 100, 50, 128], 1.0, 0)])));
    let mut sprites = Sprites::new();
    sprites.add("layer", source);
    let mut canvas = Canvas::new([2, 2]);
    canvas.place(sprites.finish(), "layer-00", [-1, -1], 0);
    let image = output(canvas.finish())?;
    assert_eq!(&image.rgba[..4], &[200, 100, 50, 128]);
    assert!(image.rgba[4..].iter().all(|b| *b == 0));
    assert!(Image::transparent([u32::MAX; 2]).is_err());
    Ok(())
}

#[test]
fn malformed_pixels_and_depth_are_errors_not_indexing_panics() {
    let mut source = frame([255; 4], 1.0, 0);
    source.image.rgba.pop();
    assert!(resolve(&source, None, [1, 1], 110, 10).is_err());
    source.image.rgba.push(255);
    source.depth.as_mut().unwrap()[0] = f32::NAN;
    assert!(source.validate().is_err());
}
