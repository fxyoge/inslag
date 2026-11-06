//! Render authored EEVEE scenes and view layers into color/depth sequences.
//!
//! The .blend owns appearance, lights, and animation. [`Grid`] selects baked
//! frame coordinates (variants in declaration order, animation innermost).
//! Set BLENDER to an installed executable matching [`Blender::new`]; generation
//! never downloads tools, and cached generation/checking never starts Blender.

use inslag_asset_image::{Coordinate, Frame, Frames, Image, annotate};
use inslag_asset_pipeline::{Asset, Context, Operation, Result, invalid};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const IMPLEMENTATION: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    "/",
    env!("INSLAG_ASSET_IMPLEMENTATION")
);

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct Blender {
    version: [u32; 3],
}
impl Blender {
    pub fn new(version: [u32; 3]) -> Self {
        Self { version }
    }
    pub fn open(&self, source: impl Into<PathBuf>) -> Blend {
        Blend {
            source: source.into(),
            version: self.version,
        }
    }
}
#[derive(Clone)]
pub struct Blend {
    source: PathBuf,
    version: [u32; 3],
}
impl Blend {
    pub fn scene(&self, name: impl Into<String>) -> Scene {
        Scene {
            blend: self.clone(),
            name: name.into(),
        }
    }
}
#[derive(Clone)]
pub struct Scene {
    blend: Blend,
    name: String,
}
impl Scene {
    pub fn sprites(&self, size: [u32; 2]) -> RenderedSprites {
        RenderedSprites {
            scene: self.clone(),
            size,
            pivot: [0, 0],
            grid: Grid::new(),
            context: None,
            layers: vec![],
            composites: vec![],
            imports: vec![],
        }
    }
    pub fn view_layer(&self, name: impl Into<String>) -> Render {
        Render {
            source: self.blend.source.clone(),
            version: self.blend.version,
            scene: self.name.clone(),
            layer: name.into(),
            frames: vec![0],
            resolution: None,
        }
    }
}

pub struct RenderedSprites {
    scene: Scene,
    size: [u32; 2],
    pivot: [i32; 2],
    grid: Grid,
    context: Option<String>,
    layers: Vec<Layer>,
    composites: Vec<Composite>,
    imports: Vec<Asset<inslag_asset_image::SpriteCollection>>,
}
pub struct Layer {
    name: String,
    view: String,
    axes: Vec<String>,
    occluded: bool,
    opacity: f32,
    fade: Option<[f32; 2]>,
}
impl Layer {
    pub fn view_layer(&mut self, name: impl Into<String>) -> &mut Self {
        self.view = name.into();
        self
    }
    pub fn occluded(&mut self) -> &mut Self {
        self.occluded = true;
        self
    }
    pub fn opacity(&mut self, value: f32) -> &mut Self {
        self.opacity = value;
        self
    }
    pub fn fade_y(&mut self, rows: [f32; 2]) -> &mut Self {
        self.fade = Some(rows);
        self
    }
}
struct Composite {
    name: String,
    coordinates: std::collections::BTreeMap<String, u32>,
    layers: Vec<String>,
}
impl RenderedSprites {
    pub fn pivot(mut self, pivot: [i32; 2]) -> Self {
        self.pivot = pivot;
        self
    }
    pub fn grid(mut self, grid: Grid) -> Self {
        self.grid = grid;
        self
    }
    pub fn context(mut self, view: impl Into<String>) -> Self {
        self.context = Some(view.into());
        self
    }
    pub fn layer(&mut self, name: impl Into<String>, axes: &[&str]) -> &mut Layer {
        let name = name.into();
        self.layers.push(Layer {
            view: name.clone(),
            name,
            axes: axes.iter().map(|s| (*s).to_owned()).collect(),
            occluded: false,
            opacity: 1.0,
            fade: None,
        });
        self.layers.last_mut().unwrap()
    }
    pub fn composite(
        &mut self,
        name: impl Into<String>,
        coordinates: &[(&str, u32)],
        layers: &[&str],
    ) -> &mut Self {
        self.composites.push(Composite {
            name: name.into(),
            coordinates: coordinates
                .iter()
                .map(|(n, v)| ((*n).to_owned(), *v))
                .collect(),
            layers: layers.iter().map(|n| (*n).to_owned()).collect(),
        });
        self
    }
    pub fn extend(&mut self, sprites: Asset<inslag_asset_image::SpriteCollection>) -> &mut Self {
        self.imports.push(sprites);
        self
    }
    pub fn finish(self) -> Result<Asset<inslag_asset_image::SpriteCollection>> {
        use inslag_asset_image::{Canvas, Pixels, Sprites, Transform};
        let resolution = self
            .size
            .map(|v| {
                v.checked_mul(2)
                    .ok_or_else(|| invalid("supersampled dimensions overflow"))
            })
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        let resolution = [resolution[0], resolution[1]];
        let axes: Vec<_> = self.grid.axes.iter().map(|(n, _)| n.as_str()).collect();
        let context = self
            .context
            .as_ref()
            .map(|name| {
                self.scene
                    .view_layer(name)
                    .resolution(resolution)
                    .samples(&self.grid, &axes)
            })
            .transpose()?;
        let mut collection = Sprites::new();
        for layer in &self.layers {
            let axes: Vec<_> = layer.axes.iter().map(String::as_str).collect();
            let raw = self
                .scene
                .view_layer(&layer.view)
                .resolution(resolution)
                .samples(&self.grid, &axes)?;
            let mut pixels = Pixels::new(raw, self.size);
            if layer.occluded {
                pixels = pixels.occluded_by(
                    context
                        .clone()
                        .ok_or_else(|| invalid("occluded layer requires a context view layer"))?,
                );
            }
            let mut image = pixels.finish();
            if layer.opacity != 1.0 || layer.fade.is_some() {
                let mut transform = Transform::new(image).opacity(layer.opacity);
                if let Some(fade) = layer.fade {
                    transform = transform.fade_y(fade);
                }
                image = transform.finish();
            }
            collection.add(&layer.name, image).pivot(self.pivot);
        }
        let rendered = collection.finish();
        let mut collection = Sprites::new();
        collection.extend(rendered.clone());
        for composite in self.composites {
            let mut canvas = Canvas::new(self.size);
            for name in composite.layers {
                let layer =
                    self.layers.iter().find(|l| l.name == name).ok_or_else(|| {
                        invalid(format!("composite references missing layer {name}"))
                    })?;
                let mut slice = name;
                for axis in layer.axes.iter().filter(|n| *n != "frame") {
                    let value = composite
                        .coordinates
                        .get(axis)
                        .copied()
                        .ok_or_else(|| invalid(format!("composite needs coordinate {axis}")))?;
                    slice.push_str(&format!("-{value:02}"));
                }
                let frame = if layer.axes.iter().any(|n| n == "frame") {
                    composite
                        .coordinates
                        .get("frame")
                        .copied()
                        .ok_or_else(|| invalid("composite needs frame coordinate"))?
                        as usize
                } else {
                    0
                };
                canvas.place(rendered.clone(), slice, [0, 0], frame);
            }
            collection
                .add(composite.name, canvas.finish())
                .pivot(self.pivot);
        }
        for imported in self.imports {
            collection.extend(imported);
        }
        Ok(collection.finish())
    }
}

#[derive(Clone, Serialize)]
pub struct Render {
    source: PathBuf,
    version: [u32; 3],
    scene: String,
    layer: String,
    frames: Vec<i32>,
    resolution: Option<[u32; 2]>,
}
impl Render {
    pub fn frames(mut self, frames: impl IntoIterator<Item = i32>) -> Self {
        self.frames = frames.into_iter().collect();
        self
    }
    pub fn resolution(mut self, size: [u32; 2]) -> Self {
        self.resolution = Some(size);
        self
    }
    pub fn samples(mut self, grid: &Grid, selected: &[&str]) -> Result<Asset<Frames>> {
        let (frames, coordinates) = grid.select(selected)?;
        self.frames = frames;
        Ok(annotate(self.finish(), coordinates))
    }
    pub fn finish(self) -> Asset<Frames> {
        Asset::new(self)
    }
}
#[derive(Deserialize)]
struct RenderReport {
    version: String,
    build: String,
    gpu: String,
    dependencies: Vec<PathBuf>,
}

impl Operation for Render {
    type Output = Frames;
    fn implementation(&self) -> &'static str {
        IMPLEMENTATION
    }
    fn sources(&self) -> Vec<PathBuf> {
        vec![self.source.clone()]
    }
    fn validate(&self) -> Result<()> {
        if self.scene.is_empty()
            || self.layer.is_empty()
            || self.frames.is_empty()
            || self.frames.len() > 4096
            || self
                .frames
                .iter()
                .any(|f| !(-1_000_000..=1_000_000).contains(f))
            || self.resolution.is_some_and(|[w, h]| {
                w == 0
                    || h == 0
                    || u64::from(w) * u64::from(h) * self.frames.len() as u64 > 256_000_000
            })
        {
            return Err(invalid(
                "invalid Blender scene, view layer, frames, or resolution",
            ));
        }
        Ok(())
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<Frames> {
        let source = ctx.source(&self.source)?;
        let job = ctx.work_dir().join("job.json");
        let worker = ctx.work_dir().join("render.py");
        fs::write(&worker, include_str!("render.py"))?;
        fs::write(
            &job,
            serde_json::to_vec(
                &serde_json::json!({"source":source,"version":self.version,"scene":self.scene,"layer":self.layer,"frames":self.frames,"resolution":self.resolution,"output":ctx.work_dir()}),
            )?,
        )?;
        let executable = std::env::var_os("BLENDER").unwrap_or_else(|| "blender".into());
        let output = Command::new(executable)
            .args([
                "--background",
                "--factory-startup",
                "--disable-autoexec",
                "--python-exit-code",
                "1",
                "--python",
            ])
            .arg(&worker)
            .arg("--")
            .arg(&job)
            .output()
            .map_err(|e| {
                invalid(format!(
                    "cannot start Blender: {e}; set BLENDER to the pinned executable"
                ))
            })?;
        if !output.status.success() {
            return Err(invalid(format!(
                "Blender render failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        let report: RenderReport =
            serde_json::from_slice(&fs::read(ctx.work_dir().join("report.json"))?)?;
        for dependency in report.dependencies {
            ctx.source(dependency)?;
        }
        ctx.record_provenance("blender.version", report.version);
        ctx.record_provenance("blender.build", report.build);
        ctx.record_provenance("blender.gpu", report.gpu);
        let mut frames = vec![];
        for index in 0..self.frames.len() {
            let image = Image::read_png(&fs::read(ctx.work_dir().join(format!("{index}.png")))?)?;
            let depth = read_depth(
                &ctx.work_dir().join(format!("{index}.exr")),
                &self.layer,
                [image.width, image.height],
            )?;
            frames.push(Frame {
                image,
                depth: Some(depth),
                variants: vec![],
                time: (self.frames.len() > 1).then_some(index as u32),
            });
        }
        let frames = Frames(frames);
        frames.validate()?;
        Ok(frames)
    }
}

#[derive(Clone)]
pub struct Grid {
    axes: Vec<(String, u32)>,
    animation: u32,
}
impl Default for Grid {
    fn default() -> Self {
        Self::new()
    }
}
impl Grid {
    pub fn new() -> Self {
        Self {
            axes: vec![],
            animation: 1,
        }
    }
    pub fn variant(mut self, name: impl Into<String>, count: u32) -> Self {
        self.axes.push((name.into(), count));
        self
    }
    pub fn animation(mut self, count: u32) -> Self {
        self.animation = count;
        self
    }
    fn select(&self, selected: &[&str]) -> Result<(Vec<i32>, Vec<Coordinate>)> {
        let mut names = std::collections::BTreeSet::new();
        let mut total = self.animation as usize;
        if self.animation == 0 {
            return Err(invalid("animation sample count must be positive"));
        }
        for (name, count) in &self.axes {
            if name.is_empty() || name == "frame" || *count == 0 || !names.insert(name.as_str()) {
                return Err(invalid("sample axes need unique names and positive counts"));
            }
            total = total
                .checked_mul(*count as usize)
                .ok_or_else(|| invalid("sample grid overflow"))?;
        }
        if total > 4096
            || selected
                .iter()
                .any(|name| *name != "frame" && !names.contains(name))
            || selected
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != selected.len()
        {
            return Err(invalid(
                "invalid selected axes or sample grid exceeds 4096 frames",
            ));
        }
        let mut frames = vec![];
        let mut coordinates = vec![];
        for index in 0..total {
            let time = index % self.animation as usize;
            let mut value = index / self.animation as usize;
            let mut values = std::collections::BTreeMap::new();
            for (name, count) in self.axes.iter().rev() {
                values.insert(name.as_str(), (value % *count as usize) as u32);
                value /= *count as usize;
            }
            if (!selected.contains(&"frame") && time != 0)
                || values
                    .iter()
                    .any(|(name, value)| !selected.contains(name) && *value != 0)
            {
                continue;
            }
            frames.push(index as i32);
            coordinates.push(Coordinate {
                variants: selected
                    .iter()
                    .filter(|name| **name != "frame")
                    .map(|name| ((*name).to_owned(), values[name]))
                    .collect(),
                time: selected.contains(&"frame").then_some(time as u32),
            });
        }
        Ok((frames, coordinates))
    }
}

fn read_depth(path: &Path, name: &str, [width, height]: [u32; 2]) -> Result<Vec<f32>> {
    use exr::prelude::*;
    let image = read()
        .no_deep_data()
        .largest_resolution_level()
        .all_channels()
        .all_layers()
        .all_attributes()
        .from_file(path)?;
    let layer_name = format!("{name}.Depth");
    let layer = image
        .layer_data
        .iter()
        .find(|l| {
            l.attributes
                .layer_name
                .as_ref()
                .is_some_and(|n| n.to_string() == layer_name)
        })
        .ok_or_else(|| invalid(format!("missing depth layer {layer_name}")))?;
    if layer.size.width() != width as usize || layer.size.height() != height as usize {
        return Err(invalid("Blender color/depth dimensions differ"));
    }
    let channel = layer
        .channel_data
        .list
        .iter()
        .find(|c| c.name.to_string() == format!("{name}.Depth.Z"))
        .ok_or_else(|| invalid("missing depth channel"))?;
    Ok(channel.sample_data.values_as_f32().collect())
}
