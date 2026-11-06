use inslag_asset_image::*;

fn frame(width: u32, height: u32, pixels: Vec<u8>, depth: Vec<f32>) -> Frame {
    Frame {
        image: Image {
            width,
            height,
            rgba: pixels,
        },
        depth: Some(depth),
        variants: vec![],
        time: None,
    }
}

#[test]
fn hard_coverage_and_palette_are_native_pixel_operations() {
    let source = frame(2, 2, [153, 128, 251, 255].repeat(4), vec![1.0; 4]);
    let output = resolve(&source, None, [1, 1], 110, 10).unwrap();
    assert_eq!(output.image.rgba, [150, 130, 250, 255]);
    assert_eq!(output.depth.unwrap(), [1.0]);
}

#[test]
fn nearest_native_occluder_owns_the_whole_pixel() {
    let foreground = frame(
        2,
        2,
        vec![
            100, 100, 100, 255, 0, 0, 0, 0, 100, 100, 100, 255, 0, 0, 0, 0,
        ],
        vec![1.0, 1e10, 1.0, 1e10],
    );
    let moving = frame(2, 2, [250, 200, 0, 255].repeat(4), vec![2.0; 4]);
    assert_eq!(
        resolve(&moving, Some(&foreground), [1, 1], 110, 10)
            .unwrap()
            .image
            .rgba[3],
        0
    );
    let behind = frame(2, 2, [100, 100, 100, 255].repeat(4), vec![3.0; 4]);
    assert_eq!(
        resolve(&moving, Some(&behind), [1, 1], 110, 10)
            .unwrap()
            .image
            .rgba[3],
        255
    );
    assert_eq!(foreground.image.rgba[3], 255);
}

#[test]
fn empty_samples_do_not_darken_opaque_edges() {
    let image = frame(
        2,
        2,
        vec![200, 100, 50, 255, 0, 0, 0, 0, 200, 100, 50, 255, 0, 0, 0, 0],
        vec![1.0; 4],
    );
    assert_eq!(
        resolve(&image, None, [1, 1], 110, 10).unwrap().image.rgba,
        [200, 100, 50, 255]
    );
    assert!(resolve(&image, None, [3, 3], 110, 10).is_err());
}
