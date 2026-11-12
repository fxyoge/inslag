use inslag_asset_aseprite::{Binding, Sheet};
use inslag_asset_kui::Stylesheet;
use inslag_asset_pipeline::{Asset, Context, Operation, Pipeline, Result};
use serde::Serialize;
use std::{collections::BTreeMap, fs};

#[derive(Serialize)]
struct Fixture {
    role: String,
}
impl Operation for Fixture {
    type Output = Sheet;
    fn implementation(&self) -> &'static str {
        "fixture"
    }
    fn execute(&self, _: &Context<'_>) -> Result<Sheet> {
        Ok(Sheet {
            data: vec![1, 2],
            bindings: vec![Binding {
                name: "rotor-03".into(),
                role: self.role.clone(),
                attributes: BTreeMap::from([("pose".into(), 3)]),
                scale: 0.25,
                frame_size: [11, 12],
                frame_count: 4,
            }],
        })
    }
}

#[test]
fn published_stylesheets_derive_relative_urls_variants_and_frames() -> Result<()> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("Cargo.lock"), "version=4\n")?;
    let mut p = Pipeline::new(root.path())?;
    let sheet = p.export(
        "textures/sheet.aseprite",
        Asset::new(Fixture {
            role: "rotor".into(),
        }),
    )?;
    Stylesheet::new(&sheet)
        .prefix("fan")
        .publish(&mut p, "ui/fan/style.css")?;
    p.generate()?;
    let css = fs::read_to_string(root.path().join("assets/generated/ui/fan/style.css"))?;
    assert!(css.contains(".fan-rotor[pose=\"3\"]"));
    assert!(css.contains("url(\"../../textures/sheet.aseprite\") rotor-03"));
    assert!(css.contains("-aseprite-crop: 0px 0px 11px 12px"));
    assert!(css.contains(".fan-rotor[frame=\"3\"] { -sprite-frame: 3; }"));
    assert!(css.contains("-aseprite-scale: 0.25"));
    Ok(())
}

#[test]
fn invalid_selectors_fail_instead_of_injecting_css() -> Result<()> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("Cargo.lock"), "version=4\n")?;
    let mut p = Pipeline::new(root.path())?;
    let sheet = p.export(
        "sheet.aseprite",
        Asset::new(Fixture {
            role: "x { color: red }".into(),
        }),
    )?;
    Stylesheet::new(&sheet)
        .prefix("fan")
        .publish(&mut p, "style.css")?;
    assert!(p.generate().is_err());
    assert!(!root.path().join("assets/generated/style.css").exists());
    Ok(())
}
