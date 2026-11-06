//! Rasterize self-contained SVG sources without a system font dependency.
//! Images must be embedded; external file references are rejected rather than
//! silently producing incomplete or untracked output.

use inslag_asset_image::{Frame, Frames, Image};
use inslag_asset_pipeline::{Asset, Context, Operation, Result, invalid};
use serde::Serialize;
use std::{fs, path::PathBuf};

#[derive(Serialize)]
pub struct Svg {
    path: PathBuf,
}
impl Svg {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn finish(self) -> Asset<Frames> {
        Asset::new(self)
    }
}
impl Operation for Svg {
    type Output = Frames;
    fn implementation(&self) -> &'static str {
        concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("INSLAG_ASSET_IMPLEMENTATION")
        )
    }
    fn sources(&self) -> Vec<PathBuf> {
        vec![self.path.clone()]
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<Frames> {
        let bytes = fs::read(ctx.source(&self.path)?)?;
        let rejected = std::sync::atomic::AtomicBool::new(false);
        let mut options = resvg::usvg::Options::default();
        options.image_href_resolver.resolve_string = Box::new(|_, _| {
            rejected.store(true, std::sync::atomic::Ordering::Relaxed);
            None
        });
        let tree = resvg::usvg::Tree::from_data(&bytes, &options)?;
        if rejected.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(invalid(
                "SVG images must be embedded, not external file references",
            ));
        }
        let size = tree.size().to_int_size();
        if u64::from(size.width()) * u64::from(size.height()) > 64_000_000 {
            return Err(invalid("SVG canvas exceeds 64M pixels"));
        }
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
            .ok_or_else(|| invalid("invalid SVG canvas"))?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut pixmap.as_mut(),
        );
        let rgba = pixmap
            .pixels()
            .iter()
            .flat_map(|pixel| {
                let p = pixel.demultiply();
                [p.red(), p.green(), p.blue(), p.alpha()]
            })
            .collect();
        Ok(Frames(vec![Frame {
            image: Image {
                width: size.width(),
                height: size.height(),
                rgba,
            },
            depth: None,
            variants: vec![],
            time: None,
        }]))
    }
}
