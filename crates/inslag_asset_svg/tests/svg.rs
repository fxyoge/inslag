use inslag_asset_image::{Transform, png};
use inslag_asset_pipeline::{Pipeline, Result};
use inslag_asset_svg::Svg;
use std::fs;

#[test]
fn vectors_preserve_straight_alpha_and_pipeline_check_is_processor_free() -> Result<()> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("Cargo.lock"), "lock")?;
    fs::write(
        root.path().join("source.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><path fill="red" fill-opacity="0.5" d="M0 0H2V2H0Z"/></svg>"#,
    )?;
    let pipeline = || -> Result<Pipeline> {
        let mut pipeline = Pipeline::new(root.path())?;
        pipeline.export("image.png", png(Svg::open("source.svg").finish(), 0))?;
        Ok(pipeline)
    };
    pipeline()?.generate()?;
    let image = inslag_asset_image::Image::read_png(&fs::read(
        root.path().join("assets/generated/image.png"),
    )?)?;
    assert_eq!(&image.rgba[..4], &[255, 0, 0, 128]);
    pipeline()?.check()?;
    Ok(())
}

#[test]
fn external_images_and_unbounded_fit_can_never_be_silently_accepted() -> Result<()> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("Cargo.lock"), "lock")?;
    fs::write(
        root.path().join("source.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2"><image href="../../outside.png" width="2" height="2"/></svg>"#,
    )?;
    let mut p = Pipeline::new(root.path())?;
    p.export("image.png", png(Svg::open("source.svg").finish(), 0))?;
    assert!(p.generate().unwrap_err().to_string().contains("embedded"));
    let mut p = Pipeline::new(root.path())?;
    p.export(
        "image.png",
        png(
            Transform::new(Svg::open("source.svg").finish())
                .fit([u32::MAX; 2])
                .finish(),
            0,
        ),
    )?;
    assert!(p.generate().is_err());
    Ok(())
}
