//! Import named Aseprite slices and publish packed sprite collections.
//!
//! Animation is packed as horizontal strips inside a single Aseprite frame;
//! bindings retain native frame sizes, pivots, scale, and selectors. Aliases
//! share packed pixels. Packing does not require the Aseprite executable.

use inslag_asset_image::{Image, Sprite, SpriteCollection};
use inslag_asset_pipeline::{Artifact, Asset, Context, Dependency, Operation, Result, invalid};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Write, path::PathBuf};

pub const IMPLEMENTATION: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    "/",
    env!("INSLAG_ASSET_IMPLEMENTATION")
);

#[derive(Clone, Serialize, Deserialize)]
pub struct Binding {
    pub name: String,
    pub role: String,
    pub attributes: BTreeMap<String, u32>,
    pub scale: f32,
    pub frame_size: [u32; 2],
    pub frame_count: usize,
}
#[derive(Serialize, Deserialize)]
pub struct Sheet {
    pub data: Vec<u8>,
    pub bindings: Vec<Binding>,
}
impl Artifact for Sheet {
    fn bytes(&self) -> &[u8] {
        &self.data
    }
}

#[derive(Serialize)]
pub struct Aseprite {
    source: Asset<SpriteCollection>,
    max_size: [u32; 2],
}
impl Aseprite {
    pub fn new(source: Asset<SpriteCollection>) -> Self {
        Self {
            source,
            max_size: [4096, 8192],
        }
    }
    pub fn max_size(mut self, size: [u32; 2]) -> Self {
        self.max_size = size;
        self
    }
    pub fn finish(self) -> Asset<Sheet> {
        Asset::new(self)
    }
}
impl Operation for Aseprite {
    type Output = Sheet;
    fn implementation(&self) -> &'static str {
        IMPLEMENTATION
    }
    fn dependencies(&self) -> Vec<Dependency> {
        vec![self.source.dependency()]
    }
    fn validate(&self) -> Result<()> {
        if self.max_size.iter().any(|v| *v == 0 || *v > 32767) {
            Err(invalid("Aseprite dimensions must be within 1..32767"))
        } else {
            Ok(())
        }
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<Sheet> {
        pack(ctx.input(&self.source)?.as_ref(), self.max_size)
    }
}

pub fn pack(sprites: &SpriteCollection, max_size: [u32; 2]) -> Result<Sheet> {
    sprites.validate()?;
    if sprites.0.is_empty()
        || sprites.0.len() > u16::MAX as usize
        || max_size.iter().any(|v| *v == 0 || *v > 32767)
    {
        return Err(invalid("invalid atlas size or sprite count"));
    }
    let mut area = 0u64;
    let mut widest = 1;
    for sprite in &sprites.0 {
        let width = sprite.frames[0]
            .width
            .checked_mul(u32::try_from(sprite.frames.len())?)
            .ok_or_else(|| invalid("animation strip width overflow"))?;
        widest = widest.max(width);
        area += u64::from(width) * u64::from(sprite.frames[0].height);
    }
    let width = ((area as f64).sqrt().ceil() as u32)
        .next_power_of_two()
        .max(widest)
        .min(max_size[0]);
    if widest > width {
        return Err(invalid("sprite animation exceeds atlas width"));
    }
    let (mut x, mut y, mut row_height, mut count) = (0u32, 0u32, 0u32, 0u32);
    let mut body = vec![];
    let mut bindings = vec![];
    for (index, sprite) in sprites.0.iter().enumerate() {
        let frame = &sprite.frames[0];
        let strip_width = frame.width * sprite.frames.len() as u32;
        if x + strip_width > width {
            x = 0;
            y += row_height;
            row_height = 0;
        }
        row_height = row_height.max(frame.height);
        if y + row_height > max_size[1] || u64::from(width) * u64::from(y + row_height) > 64_000_000
        {
            return Err(invalid("sprite atlas exceeds configured height"));
        }
        let mut layer = vec![];
        for value in [3, 0, 0, 0, 0, 0] {
            word(&mut layer, value);
        }
        layer.extend([255, 0, 0, 0]);
        string(&mut layer, &sprite.names[0])?;
        chunk(&mut body, 0x2004, &layer)?;
        let mut strip = vec![0; (u64::from(strip_width) * u64::from(frame.height) * 4).try_into()?];
        for (frame_index, image) in sprite.frames.iter().enumerate() {
            for row in 0..image.height as usize {
                let length = image.width as usize * 4;
                let offset = row * strip_width as usize * 4 + frame_index * length;
                strip[offset..offset + length]
                    .copy_from_slice(&image.rgba[row * length..(row + 1) * length]);
            }
        }
        let mut cel = vec![];
        for value in [index as u16, x as u16, y as u16] {
            word(&mut cel, value);
        }
        cel.push(255);
        word(&mut cel, 2);
        cel.extend([0; 7]);
        word(&mut cel, strip_width as u16);
        word(&mut cel, frame.height as u16);
        let mut encoder = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
        encoder.write_all(&strip)?;
        cel.extend(encoder.finish()?);
        chunk(&mut body, 0x2005, &cel)?;
        count += 2;
        for name in &sprite.names {
            let mut slice = vec![];
            for value in [1, 2, 0] {
                dword(&mut slice, value);
            }
            string(&mut slice, name)?;
            for value in [0, x, y, strip_width, frame.height] {
                dword(&mut slice, value);
            }
            for value in sprite.pivot {
                slice.extend(value.to_le_bytes());
            }
            chunk(&mut body, 0x2022, &slice)?;
            count += 1;
            bindings.push(Binding {
                name: name.clone(),
                role: sprite.role.clone().unwrap_or_else(|| name.clone()),
                attributes: sprite.attributes.clone(),
                scale: sprite.scale,
                frame_size: [frame.width, frame.height],
                frame_count: sprite.frames.len(),
            });
        }
        x += strip_width;
    }
    let mut frame = vec![];
    dword(&mut frame, u32::try_from(body.len() + 16)?);
    word(&mut frame, 0xF1FA);
    word(&mut frame, count.min(u16::MAX as u32) as u16);
    word(&mut frame, 100);
    word(&mut frame, 0);
    dword(&mut frame, count);
    frame.extend(body);
    let mut data = vec![];
    dword(&mut data, u32::try_from(128 + frame.len())?);
    for value in [0xA5E0, 1, width as u16, (y + row_height) as u16, 32] {
        word(&mut data, value);
    }
    dword(&mut data, 1);
    word(&mut data, 100);
    data.resize(128, 0);
    data[28] = 1;
    data[34] = 1;
    data[35] = 1;
    data.extend(frame);
    Ok(Sheet { data, bindings })
}

#[derive(Serialize)]
struct Import {
    path: PathBuf,
}
pub fn import(path: impl Into<PathBuf>) -> Asset<SpriteCollection> {
    Asset::new(Import { path: path.into() })
}
impl Operation for Import {
    type Output = SpriteCollection;
    fn implementation(&self) -> &'static str {
        IMPLEMENTATION
    }
    fn sources(&self) -> Vec<PathBuf> {
        vec![self.path.clone()]
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<SpriteCollection> {
        let file = asefile::AsepriteFile::read_file(&ctx.source(&self.path)?)?;
        if file.slices().is_empty() || file.width() * file.height() > 64_000_000 {
            return Err(invalid(
                "Aseprite source needs named slices within a 64M pixel canvas",
            ));
        }
        let rendered: Vec<_> = (0..file.num_frames())
            .map(|i| file.frame(i).image())
            .collect();
        let mut sprites = vec![];
        for slice in file.slices() {
            let mut images = vec![];
            let mut pivot = None;
            for (index, canvas) in rendered.iter().enumerate() {
                let key = slice
                    .keys
                    .iter()
                    .filter(|key| key.from_frame <= index as u32)
                    .max_by_key(|key| key.from_frame)
                    .ok_or_else(|| invalid("slice has no initial key"))?;
                if key.slice9.is_some() {
                    return Err(invalid("nine-patch source slices are not supported"));
                }
                let (x, y) = key.origin;
                let (width, height) = key.size;
                if x < 0
                    || y < 0
                    || width == 0
                    || height == 0
                    || x as u64 + u64::from(width) > file.width() as u64
                    || y as u64 + u64::from(height) > file.height() as u64
                {
                    return Err(invalid("Aseprite slice outside canvas"));
                }
                let current_pivot = key.pivot.unwrap_or((width as i32 / 2, height as i32 / 2));
                if pivot.is_some_and(|p| p != current_pivot) {
                    return Err(invalid("animated pivots require separate sprites"));
                }
                pivot = Some(current_pivot);
                let mut rgba = Vec::with_capacity((width * height * 4) as usize);
                for row in 0..height {
                    for col in 0..width {
                        rgba.extend(canvas.get_pixel(x as u32 + col, y as u32 + row).0);
                    }
                }
                images.push(Image {
                    width,
                    height,
                    rgba,
                });
            }
            let (x, y) = pivot.unwrap();
            sprites.push(Sprite {
                names: vec![slice.name.clone()],
                frames: images,
                pivot: [x, y],
                scale: 1.0,
                attributes: BTreeMap::new(),
                role: None,
            });
        }
        let collection = SpriteCollection(sprites);
        collection.validate()?;
        Ok(collection)
    }
}

fn string(out: &mut Vec<u8>, value: &str) -> Result<()> {
    word(out, u16::try_from(value.len())?);
    out.extend(value.as_bytes());
    Ok(())
}
fn chunk(out: &mut Vec<u8>, kind: u16, bytes: &[u8]) -> Result<()> {
    dword(out, u32::try_from(bytes.len() + 6)?);
    word(out, kind);
    out.extend(bytes);
    Ok(())
}

fn word(out: &mut Vec<u8>, value: u16) {
    out.extend(value.to_le_bytes());
}

fn dword(out: &mut Vec<u8>, value: u32) {
    out.extend(value.to_le_bytes());
}
