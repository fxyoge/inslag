use super::{Grid, read_depth};
use inslag_asset_pipeline::Result;

#[test]
fn grid_projects_baked_frames_but_preserves_requested_variant_order() -> Result<()> {
    let grid = Grid::new()
        .variant("pose", 3)
        .variant("door", 2)
        .animation(4);
    let (frames, coordinates) = grid.select(&["pose", "frame"])?;
    assert_eq!(frames, [0, 1, 2, 3, 8, 9, 10, 11, 16, 17, 18, 19]);
    assert_eq!(coordinates[4].variants, [("pose".into(), 1)]);
    assert_eq!(coordinates[4].time, Some(0));
    let (frames, coordinates) = grid.select(&["door", "pose"])?;
    assert_eq!(frames, [0, 4, 8, 12, 16, 20]);
    assert_eq!(
        coordinates[3].variants,
        [("door".into(), 1), ("pose".into(), 1)]
    );
    assert_eq!(coordinates[3].time, None);
    Ok(())
}

#[test]
fn invalid_grids_fail_before_rendering() {
    for grid in [
        Grid::new().animation(0),
        Grid::new().variant("pose", 0),
        Grid::new().variant("pose", u32::MAX).animation(u32::MAX),
        Grid::new().variant("pose", 2).variant("pose", 3),
        Grid::new().variant("frame", 2),
    ] {
        assert!(grid.select(&[]).is_err());
    }
    assert!(Grid::new().select(&["unknown"]).is_err());
    assert!(Grid::new().select(&["frame", "frame"]).is_err());
}

#[test]
fn depth_reader_selects_exact_named_pass_not_a_stale_first_part() -> Result<()> {
    use exr::prelude::*;
    let root = tempfile::tempdir()?;
    let path = root.path().join("depth.exr");
    let stale = Layer::new(
        (2, 1),
        LayerAttributes::named("static.Depth"),
        Encoding::FAST_LOSSLESS,
        SpecificChannels::build()
            .with_channel("static.Depth.Z")
            .with_pixel_fn(|_| (99_f32,)),
    );
    let wanted = Layer::new(
        (2, 1),
        LayerAttributes::named("rotor.Depth"),
        Encoding::FAST_LOSSLESS,
        SpecificChannels::build()
            .with_channel("rotor.Depth.Z")
            .with_pixel_fn(|p| (p.0 as f32 + 1.0,)),
    );
    Image::empty(ImageAttributes::new(IntegerBounds::from_dimensions((2, 1))))
        .with_layer(stale)
        .with_layer(wanted)
        .write()
        .to_file(&path)?;
    assert_eq!(read_depth(&path, "rotor", [2, 1])?, [1.0, 2.0]);
    assert!(read_depth(&path, "rotor", [1, 1]).is_err());
    assert!(read_depth(&path, "missing", [2, 1]).is_err());
    Ok(())
}
