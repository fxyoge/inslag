//! Publish KUI sprite stylesheets from typed packed-sheet bindings.
//! URLs are derived from the two publication paths, not the processor cache.

use inslag_asset_aseprite::Sheet;
use inslag_asset_pipeline::{
    Asset, Bytes, Context, Dependency, Operation, Pipeline, Published, Result, invalid,
};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

#[derive(Serialize)]
pub struct Stylesheet {
    source: Asset<Sheet>,
    sheet_path: PathBuf,
    path: PathBuf,
    prefix: String,
}
impl Stylesheet {
    pub fn new(sheet: &Published<Sheet>) -> Self {
        Self {
            source: sheet.asset.clone(),
            sheet_path: sheet.path.clone(),
            path: PathBuf::new(),
            prefix: String::new(),
        }
    }
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }
    pub fn publish(
        mut self,
        pipeline: &mut Pipeline,
        path: impl Into<PathBuf>,
    ) -> Result<Published<Bytes>> {
        self.path = path.into();
        pipeline.export(self.path.clone(), Asset::new(self))
    }
}
impl Operation for Stylesheet {
    type Output = Bytes;
    fn implementation(&self) -> &'static str {
        concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("INSLAG_ASSET_IMPLEMENTATION")
        )
    }
    fn dependencies(&self) -> Vec<Dependency> {
        vec![self.source.dependency()]
    }
    fn validate(&self) -> Result<()> {
        if !identifier(&self.prefix) {
            return Err(invalid(
                "stylesheet prefix must be a nonempty CSS identifier",
            ));
        }
        for path in [&self.path, &self.sheet_path] {
            if path
                .components()
                .any(|p| !matches!(p, Component::Normal(_)))
            {
                return Err(invalid("stylesheet paths must be relative"));
            }
        }
        Ok(())
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<Bytes> {
        let sheet = ctx.input(&self.source)?;
        let url = serde_json::to_string(&relative(
            &self.sheet_path,
            self.path.parent().unwrap_or(Path::new("")),
        ))?;
        let mut text = String::new();
        let mut animations = BTreeSet::new();
        for binding in &sheet.bindings {
            if !identifier(&binding.name)
                || !identifier(&binding.role)
                || !binding.scale.is_finite()
                || binding.scale <= 0.0
                || binding.frame_count == 0
                || binding.frame_size.contains(&0)
            {
                return Err(invalid("invalid sprite stylesheet binding"));
            }
            let mut selector = format!(".{}-{}", self.prefix, binding.role);
            for (attribute, value) in &binding.attributes {
                if !identifier(attribute) {
                    return Err(invalid("invalid sprite attribute"));
                }
                selector.push_str(&format!("[{attribute}=\"{value}\"]"));
            }
            text.push_str(&format!(
                "{selector} {{ -aseprite-image: url({url}) {}; -aseprite-scale: {};",
                binding.name, binding.scale
            ));
            if binding.frame_count > 1 {
                let [width, height] = binding.frame_size;
                text.push_str(&format!(" -aseprite-crop: 0px 0px {width}px {height}px;"));
                animations.insert((binding.role.clone(), binding.frame_count));
            }
            text.push_str(" }\n");
        }
        for (role, count) in animations {
            for frame in 0..count {
                text.push_str(&format!(
                    ".{}-{role}[frame=\"{frame}\"] {{ -sprite-frame: {frame}; }}\n",
                    self.prefix
                ));
            }
        }
        Ok(Bytes(text.into_bytes()))
    }
}

fn identifier(value: &str) -> bool {
    value
        .bytes()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}

fn relative(target: &Path, directory: &Path) -> String {
    let target: Vec<_> = target.components().collect();
    let directory: Vec<_> = directory.components().collect();
    let common = target
        .iter()
        .zip(&directory)
        .take_while(|(a, b)| a == b)
        .count();
    std::iter::repeat_n("..".to_owned(), directory.len() - common)
        .chain(
            target[common..]
                .iter()
                .map(|c| c.as_os_str().to_string_lossy().into_owned()),
        )
        .collect::<Vec<_>>()
        .join("/")
}
