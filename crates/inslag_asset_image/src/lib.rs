//! CPU image sequences, native-pixel ownership, transforms, and compositions.
//!
//! [`Pixels`] resolves integer supersampling with alpha-weighted color and
//! binary coverage. Optional depth context assigns each native pixel to its
//! nearest covered surface. Coordinate annotations remain separate operations
//! so naming/metadata changes need not rerender source imagery.

use inslag_asset_pipeline::{Asset, Bytes, Context, Dependency, Operation, Result, invalid};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
};

pub const IMPLEMENTATION: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    "/",
    env!("INSLAG_ASSET_IMPLEMENTATION")
);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
impl Image {
    pub fn transparent([width, height]: [u32; 2]) -> Result<Self> {
        let length = rgba_length([width, height])?;
        Ok(Self {
            width,
            height,
            rgba: vec![0; length],
        })
    }
    pub fn validate(&self) -> Result<()> {
        let count = u64::from(self.width) * u64::from(self.height);
        if count == 0 || count > 64_000_000 || count * 4 != self.rgba.len() as u64 {
            return Err(invalid("image needs 1..=64M RGBA pixels"));
        }
        Ok(())
    }
    pub fn png(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut bytes = Cursor::new(vec![]);
        image::write_buffer_with_format(
            &mut bytes,
            &self.rgba,
            self.width,
            self.height,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )?;
        Ok(bytes.into_inner())
    }
    pub fn read_png(bytes: &[u8]) -> Result<Self> {
        let mut reader =
            image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(256_000_000);
        reader.limits(limits);
        let image = reader.decode()?.into_rgba8();
        rgba_length([image.width(), image.height()])?;
        Ok(Self {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        })
    }
}

fn rgba_length([width, height]: [u32; 2]) -> Result<usize> {
    let count = u64::from(width) * u64::from(height);
    if count == 0 || count > 64_000_000 {
        return Err(invalid("image needs 1..=64M RGBA pixels"));
    }
    Ok(usize::try_from(count * 4)?)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Frame {
    pub image: Image,
    pub depth: Option<Vec<f32>>,
    pub variants: Vec<(String, u32)>,
    pub time: Option<u32>,
}
impl Frame {
    pub fn validate(&self) -> Result<()> {
        self.image.validate()?;
        if let Some(depth) = &self.depth
            && (depth.len() != (self.image.width * self.image.height) as usize
                || depth.iter().any(|z| !z.is_finite() || *z < 0.0))
        {
            return Err(invalid(
                "depth must contain one finite positive camera distance per pixel",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Frames(pub Vec<Frame>);
impl Frames {
    pub fn validate(&self) -> Result<()> {
        let first = self
            .0
            .first()
            .ok_or_else(|| invalid("expected a nonempty image sequence"))?;
        for frame in &self.0 {
            frame.validate()?;
            if (frame.image.width, frame.image.height) != (first.image.width, first.image.height) {
                return Err(invalid("sequence frame dimensions differ"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Coordinate {
    pub variants: Vec<(String, u32)>,
    pub time: Option<u32>,
}
#[derive(Serialize)]
struct Annotate {
    source: Asset<Frames>,
    coordinates: Vec<Coordinate>,
}
pub fn annotate(source: Asset<Frames>, coordinates: Vec<Coordinate>) -> Asset<Frames> {
    Asset::new(Annotate {
        source,
        coordinates,
    })
}
impl Operation for Annotate {
    type Output = Frames;
    fn implementation(&self) -> &'static str {
        IMPLEMENTATION
    }
    fn dependencies(&self) -> Vec<Dependency> {
        vec![self.source.dependency()]
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<Frames> {
        let source = ctx.input(&self.source)?;
        if source.0.len() != self.coordinates.len() {
            return Err(invalid("sample coordinate count differs from frame count"));
        }
        Ok(Frames(
            source
                .0
                .iter()
                .zip(&self.coordinates)
                .map(|(frame, coordinate)| Frame {
                    image: frame.image.clone(),
                    depth: frame.depth.clone(),
                    variants: coordinate.variants.clone(),
                    time: coordinate.time,
                })
                .collect(),
        ))
    }
}

#[derive(Clone, Serialize)]
pub struct Pixels {
    source: Asset<Frames>,
    context: Option<Asset<Frames>>,
    size: [u32; 2],
    cutoff: u8,
    palette: u8,
}
impl Pixels {
    pub fn new(source: Asset<Frames>, size: [u32; 2]) -> Self {
        Self {
            source,
            context: None,
            size,
            cutoff: 110,
            palette: 10,
        }
    }
    pub fn occluded_by(mut self, context: Asset<Frames>) -> Self {
        self.context = Some(context);
        self
    }
    pub fn alpha_cutoff(mut self, value: u8) -> Self {
        self.cutoff = value;
        self
    }
    pub fn palette_step(mut self, value: u8) -> Self {
        self.palette = value;
        self
    }
    pub fn finish(self) -> Asset<Frames> {
        Asset::new(self)
    }
}
impl Operation for Pixels {
    type Output = Frames;
    fn implementation(&self) -> &'static str {
        IMPLEMENTATION
    }
    fn dependencies(&self) -> Vec<Dependency> {
        std::iter::once(self.source.dependency())
            .chain(self.context.iter().map(Asset::dependency))
            .collect()
    }
    fn validate(&self) -> Result<()> {
        if self.size.contains(&0) || self.cutoff == 0 {
            Err(invalid(
                "pixel resolution and alpha cutoff must be positive",
            ))
        } else {
            Ok(())
        }
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<Frames> {
        let source = ctx.input(&self.source)?;
        source.validate()?;
        let context = self
            .context
            .as_ref()
            .map(|input| ctx.input(input))
            .transpose()?;
        if let Some(context) = &context {
            context.validate()?;
        }
        Ok(Frames(
            source
                .0
                .iter()
                .map(|frame| {
                    let context = context
                        .as_ref()
                        .map(|c| {
                            if c.0.len() == 1 {
                                return Ok(&c.0[0]);
                            }
                            let mut matching = c.0.iter().filter(|candidate| {
                                candidate.variants.iter().all(|(name, value)| {
                                    frame
                                        .variants
                                        .iter()
                                        .find(|(n, _)| n == name)
                                        .map_or(*value == 0, |(_, v)| v == value)
                                }) && (candidate.time.is_none() || candidate.time == frame.time)
                            });
                            let context = matching
                                .next()
                                .ok_or_else(|| invalid("no matching occluder sample"))?;
                            if matching.next().is_some() {
                                return Err(invalid("ambiguous occluder samples"));
                            }
                            Ok(context)
                        })
                        .transpose()?;
                    resolve(frame, context, self.size, self.cutoff, self.palette)
                })
                .collect::<Result<_>>()?,
        ))
    }
}

pub fn resolve(
    source: &Frame,
    context: Option<&Frame>,
    [width, height]: [u32; 2],
    cutoff: u8,
    palette: u8,
) -> Result<Frame> {
    source.validate()?;
    if width == 0
        || height == 0
        || cutoff == 0
        || !source.image.width.is_multiple_of(width)
        || !source.image.height.is_multiple_of(height)
    {
        return Err(invalid("native size must divide source dimensions"));
    }
    let factor = source.image.width / width;
    if factor == 0 || source.image.height / height != factor {
        return Err(invalid(
            "pixel resolve requires equal integer supersampling on both axes",
        ));
    }
    if let Some(context) = context {
        context.validate()?;
        if (source.image.width, source.image.height) != (context.image.width, context.image.height)
            || source.depth.is_none()
            || context.depth.is_none()
        {
            return Err(invalid(
                "occlusion requires matching color and depth images",
            ));
        }
    }
    let mut image = Image {
        width,
        height,
        rgba: vec![0; width as usize * height as usize * 4],
    };
    let mut depths = source
        .depth
        .as_ref()
        .map(|_| vec![f32::MAX; (width * height) as usize]);
    for y in 0..height {
        for x in 0..width {
            let mut color = [0.0f64; 3];
            let (mut alpha, mut context_alpha) = (0u64, 0u64);
            let (mut nearest, mut context_nearest) = (f32::MAX, f32::MAX);
            for sy in 0..factor {
                for sx in 0..factor {
                    let index = ((y * factor + sy) * source.image.width + x * factor + sx) as usize;
                    let source_z = source.depth.as_ref().map_or(f32::MAX, |z| z[index]);
                    nearest = nearest.min(source_z);
                    let mut a = source.image.rgba[index * 4 + 3];
                    if let Some(context) = context {
                        let z = context.depth.as_ref().unwrap()[index];
                        context_nearest = context_nearest.min(z);
                        context_alpha += u64::from(context.image.rgba[index * 4 + 3]);
                        if source_z >= z {
                            a = 0;
                        }
                    }
                    alpha += u64::from(a);
                    for (channel, total) in color.iter_mut().enumerate() {
                        let value = f64::from(source.image.rgba[index * 4 + channel]);
                        let value = if palette == 0 {
                            value
                        } else {
                            ((value / f64::from(palette)).round_ties_even() * f64::from(palette))
                                .min(255.0)
                        };
                        *total += value * f64::from(a);
                    }
                }
            }
            let index = (y * width + x) as usize;
            let samples = u64::from(factor) * u64::from(factor);
            let covered = alpha >= samples * u64::from(cutoff);
            let blocked = context.is_some()
                && context_alpha >= samples * u64::from(cutoff)
                && context_nearest <= nearest;
            if covered && !blocked {
                for (channel, total) in color.iter().enumerate() {
                    image.rgba[index * 4 + channel] =
                        (total / alpha as f64).round_ties_even() as u8;
                }
                image.rgba[index * 4 + 3] = 255;
            }
            if let Some(depths) = &mut depths {
                depths[index] = nearest;
            }
        }
    }
    Ok(Frame {
        image,
        depth: depths,
        variants: source.variants.clone(),
        time: source.time,
    })
}

#[derive(Serialize)]
struct Selection {
    source: Asset<Frames>,
    indices: Vec<usize>,
}
pub fn select(source: Asset<Frames>, indices: impl IntoIterator<Item = usize>) -> Asset<Frames> {
    Asset::new(Selection {
        source,
        indices: indices.into_iter().collect(),
    })
}
impl Operation for Selection {
    type Output = Frames;
    fn implementation(&self) -> &'static str {
        IMPLEMENTATION
    }
    fn dependencies(&self) -> Vec<Dependency> {
        vec![self.source.dependency()]
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<Frames> {
        let source = ctx.input(&self.source)?;
        Ok(Frames(
            self.indices
                .iter()
                .map(|i| {
                    source
                        .0
                        .get(*i)
                        .cloned()
                        .ok_or_else(|| invalid("frame index out of bounds"))
                })
                .collect::<Result<_>>()?,
        ))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sprite {
    pub names: Vec<String>,
    pub frames: Vec<Image>,
    pub pivot: [i32; 2],
    pub scale: f32,
    pub attributes: BTreeMap<String, u32>,
    pub role: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SpriteCollection(pub Vec<Sprite>);
impl SpriteCollection {
    pub fn validate(&self) -> Result<()> {
        let mut names = BTreeSet::new();
        for sprite in &self.0 {
            if sprite.names.is_empty()
                || sprite.frames.is_empty()
                || !sprite.scale.is_finite()
                || sprite.scale <= 0.0
            {
                return Err(invalid("sprites need names, frames, and a positive scale"));
            }
            for name in &sprite.names {
                if name.is_empty()
                    || !name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
                    || !names.insert(name)
                {
                    return Err(invalid(format!("invalid or duplicate sprite name {name}")));
                }
            }
            let first = &sprite.frames[0];
            for image in &sprite.frames {
                image.validate()?;
                if (image.width, image.height) != (first.width, first.height) {
                    return Err(invalid("sprite frame dimensions differ"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Serialize)]
pub struct SpriteSpec {
    source: Asset<Frames>,
    names: Vec<String>,
    pivot: [i32; 2],
    scale: f32,
    attributes: BTreeMap<String, u32>,
    role: Option<String>,
}
impl SpriteSpec {
    pub fn pivot(&mut self, value: [i32; 2]) -> &mut Self {
        self.pivot = value;
        self
    }
    pub fn scale(&mut self, value: f32) -> &mut Self {
        self.scale = value;
        self
    }
    pub fn aliases(&mut self, names: impl IntoIterator<Item = impl Into<String>>) -> &mut Self {
        self.names.extend(names.into_iter().map(Into::into));
        self
    }
    pub fn attribute(&mut self, name: impl Into<String>, value: u32) -> &mut Self {
        self.attributes.insert(name.into(), value);
        self
    }
    pub fn role(&mut self, name: impl Into<String>) -> &mut Self {
        self.role = Some(name.into());
        self
    }
}
#[derive(Default, Serialize)]
pub struct Sprites {
    specs: Vec<SpriteSpec>,
    collections: Vec<Asset<SpriteCollection>>,
}
impl Sprites {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, name: impl Into<String>, source: Asset<Frames>) -> &mut SpriteSpec {
        self.specs.push(SpriteSpec {
            source,
            names: vec![name.into()],
            pivot: [0, 0],
            scale: 1.0,
            attributes: BTreeMap::new(),
            role: None,
        });
        self.specs.last_mut().unwrap()
    }
    pub fn extend(&mut self, collection: Asset<SpriteCollection>) -> &mut Self {
        self.collections.push(collection);
        self
    }
    pub fn finish(self) -> Asset<SpriteCollection> {
        Asset::new(self)
    }
}
impl Operation for Sprites {
    type Output = SpriteCollection;
    fn implementation(&self) -> &'static str {
        IMPLEMENTATION
    }
    fn dependencies(&self) -> Vec<Dependency> {
        self.specs
            .iter()
            .map(|s| s.source.dependency())
            .chain(self.collections.iter().map(Asset::dependency))
            .collect()
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<SpriteCollection> {
        let mut sprites = vec![];
        for spec in &self.specs {
            let frames = ctx.input(&spec.source)?;
            frames.validate()?;
            let mut groups: BTreeMap<Vec<(String, u32)>, Vec<&Frame>> = BTreeMap::new();
            for frame in &frames.0 {
                groups
                    .entry(frame.variants.clone())
                    .or_default()
                    .push(frame);
            }
            for (variants, frames) in groups {
                let suffix = variants
                    .iter()
                    .map(|(_, v)| format!("-{v:02}"))
                    .collect::<String>();
                let mut attributes = spec.attributes.clone();
                attributes.extend(variants);
                sprites.push(Sprite {
                    names: spec.names.iter().map(|n| format!("{n}{suffix}")).collect(),
                    frames: frames.iter().map(|f| f.image.clone()).collect(),
                    pivot: spec.pivot,
                    scale: spec.scale,
                    attributes,
                    role: spec
                        .role
                        .clone()
                        .or_else(|| (!suffix.is_empty()).then(|| spec.names[0].clone())),
                });
            }
        }
        for source in &self.collections {
            sprites.extend(ctx.input(source)?.0.iter().cloned());
        }
        let sprites = SpriteCollection(sprites);
        sprites.validate()?;
        Ok(sprites)
    }
}

#[derive(Serialize)]
pub struct Transform {
    source: Asset<Frames>,
    crop: Option<[u32; 4]>,
    opacity: f32,
    fade: Option<[f32; 2]>,
    grayscale: bool,
    fit: Option<[u32; 2]>,
}
impl Transform {
    pub fn new(source: Asset<Frames>) -> Self {
        Self {
            source,
            crop: None,
            opacity: 1.0,
            fade: None,
            grayscale: false,
            fit: None,
        }
    }
    pub fn crop(mut self, rect: [u32; 4]) -> Self {
        self.crop = Some(rect);
        self
    }
    pub fn opacity(mut self, value: f32) -> Self {
        self.opacity = value;
        self
    }
    pub fn fade_y(mut self, value: [f32; 2]) -> Self {
        self.fade = Some(value);
        self
    }
    pub fn grayscale(mut self) -> Self {
        self.grayscale = true;
        self
    }
    pub fn fit(mut self, size: [u32; 2]) -> Self {
        self.fit = Some(size);
        self
    }
    pub fn finish(self) -> Asset<Frames> {
        Asset::new(self)
    }
}
impl Operation for Transform {
    type Output = Frames;
    fn implementation(&self) -> &'static str {
        IMPLEMENTATION
    }
    fn dependencies(&self) -> Vec<Dependency> {
        vec![self.source.dependency()]
    }
    fn validate(&self) -> Result<()> {
        if let Some(size) = self.fit {
            rgba_length(size)?;
        }
        if !self.opacity.is_finite()
            || !(0.0..=1.0).contains(&self.opacity)
            || self
                .fade
                .is_some_and(|f| f.iter().any(|v| !v.is_finite()) || f[0] == f[1])
            || self.fit.is_some_and(|s| s.contains(&0))
        {
            return Err(invalid("invalid image opacity, fade, or fit dimensions"));
        }
        Ok(())
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<Frames> {
        let frames = ctx.input(&self.source)?;
        frames.validate()?;
        let mut result = vec![];
        for frame in &frames.0 {
            let image = &frame.image;
            let source =
                image::RgbaImage::from_raw(image.width, image.height, image.rgba.clone()).unwrap();
            let mut output = if let Some([x, y, width, height]) = self.crop {
                if width == 0
                    || height == 0
                    || u64::from(x) + u64::from(width) > u64::from(image.width)
                    || u64::from(y) + u64::from(height) > u64::from(image.height)
                {
                    return Err(invalid("crop outside source image"));
                }
                image::imageops::crop_imm(&source, x, y, width, height).to_image()
            } else {
                source
            };
            if let Some([width, height]) = self.fit {
                let scale = (width as f64 / f64::from(output.width()))
                    .min(height as f64 / f64::from(output.height()))
                    .min(1.0);
                let resized = image::imageops::resize(
                    &output,
                    (f64::from(output.width()) * scale).round().max(1.0) as u32,
                    (f64::from(output.height()) * scale).round().max(1.0) as u32,
                    image::imageops::FilterType::Nearest,
                );
                output = image::RgbaImage::new(width, height);
                image::imageops::replace(
                    &mut output,
                    &resized,
                    i64::from((width - resized.width()) / 2),
                    i64::from((height - resized.height()) / 2),
                );
            }
            for (y, row) in output.rows_mut().enumerate() {
                let factor = self.opacity
                    * self.fade.map_or(1.0, |[start, end]| {
                        ((y as f32 - start) / (end - start)).clamp(0.0, 1.0)
                    });
                for pixel in row {
                    if self.grayscale {
                        let gray = (0.299 * f32::from(pixel[0])
                            + 0.587 * f32::from(pixel[1])
                            + 0.114 * f32::from(pixel[2]))
                        .round() as u8;
                        pixel.0[..3].fill(gray);
                    }
                    pixel[3] = (f32::from(pixel[3]) * factor).round_ties_even() as u8;
                }
            }
            result.push(Frame {
                image: Image {
                    width: output.width(),
                    height: output.height(),
                    rgba: output.into_raw(),
                },
                depth: None,
                variants: frame.variants.clone(),
                time: frame.time,
            });
        }
        Ok(Frames(result))
    }
}

#[derive(Serialize)]
pub struct Canvas {
    size: [u32; 2],
    layers: Vec<(Asset<SpriteCollection>, String, [i32; 2], usize)>,
}
impl Canvas {
    pub fn new(size: [u32; 2]) -> Self {
        Self {
            size,
            layers: vec![],
        }
    }
    pub fn place(
        &mut self,
        source: Asset<SpriteCollection>,
        name: impl Into<String>,
        position: [i32; 2],
        frame: usize,
    ) -> &mut Self {
        self.layers.push((source, name.into(), position, frame));
        self
    }
    pub fn finish(self) -> Asset<Frames> {
        Asset::new(self)
    }
}
impl Operation for Canvas {
    type Output = Frames;
    fn implementation(&self) -> &'static str {
        IMPLEMENTATION
    }
    fn dependencies(&self) -> Vec<Dependency> {
        self.layers.iter().map(|l| l.0.dependency()).collect()
    }
    fn validate(&self) -> Result<()> {
        rgba_length(self.size).map(|_| ())
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<Frames> {
        let [width, height] = self.size;
        let mut image = Image::transparent(self.size)?;
        for (source, name, [x, y], frame) in &self.layers {
            let sprites = ctx.input(source)?;
            sprites.validate()?;
            let sprite = sprites
                .0
                .iter()
                .find(|s| s.names.contains(name))
                .ok_or_else(|| invalid(format!("missing sprite {name}")))?;
            let layer = sprite
                .frames
                .get(*frame)
                .ok_or_else(|| invalid("composite frame out of bounds"))?;
            for sy in 0..layer.height {
                for sx in 0..layer.width {
                    let (dx, dy) = (i64::from(*x) + i64::from(sx), i64::from(*y) + i64::from(sy));
                    if dx < 0 || dy < 0 || dx >= i64::from(width) || dy >= i64::from(height) {
                        continue;
                    }
                    let src = ((sy * layer.width + sx) * 4) as usize;
                    let dst = ((dy as u32 * width + dx as u32) * 4) as usize;
                    let a = f32::from(layer.rgba[src + 3]) / 255.0;
                    let b = f32::from(image.rgba[dst + 3]) / 255.0;
                    let alpha = a + b * (1.0 - a);
                    if alpha == 0.0 {
                        continue;
                    }
                    for c in 0..3 {
                        image.rgba[dst + c] = ((f32::from(layer.rgba[src + c]) * a
                            + f32::from(image.rgba[dst + c]) * b * (1.0 - a))
                            / alpha)
                            .round() as u8;
                    }
                    image.rgba[dst + 3] = (alpha * 255.0).round() as u8;
                }
            }
        }
        Ok(Frames(vec![Frame {
            image,
            depth: None,
            variants: vec![],
            time: None,
        }]))
    }
}

#[derive(Serialize)]
struct Png {
    source: Asset<Frames>,
    frame: usize,
}
pub fn png(source: Asset<Frames>, frame: usize) -> Asset<Bytes> {
    Asset::new(Png { source, frame })
}
impl Operation for Png {
    type Output = Bytes;
    fn implementation(&self) -> &'static str {
        IMPLEMENTATION
    }
    fn dependencies(&self) -> Vec<Dependency> {
        vec![self.source.dependency()]
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<Bytes> {
        Ok(Bytes(
            ctx.input(&self.source)?
                .0
                .get(self.frame)
                .ok_or_else(|| invalid("PNG frame out of bounds"))?
                .image
                .png()?,
        ))
    }
}
