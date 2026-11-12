//! Explicit, lazy offline asset graphs with content-addressed receipts.
//!
//! Export typed operation results with [`Pipeline::export`], then explicitly
//! [`Pipeline::generate`] them. [`Pipeline::check`] verifies published files,
//! configuration, implementations, lockfile, and sources without a cache or
//! executing processors. Projects must have a Cargo.lock. Custom processor
//! crates can fingerprint their implementation with [`fingerprint_package`]
//! from build.rs; declare every input through [`Operation::dependencies`].

mod fingerprint;
mod lockfile;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    marker::PhantomData,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

pub use fingerprint::{fingerprint_package, fingerprint_package_with_dependencies};
pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

const IMPLEMENTATION: &str = env!("INSLAG_ASSET_IMPLEMENTATION");
const MANIFEST: &str = ".inslag-assets.json";

pub trait Operation: Serialize + Send + Sync + 'static {
    type Output: Serialize + DeserializeOwned + Send + Sync + 'static;
    fn implementation(&self) -> &'static str;
    fn dependencies(&self) -> Vec<Dependency> {
        vec![]
    }
    fn sources(&self) -> Vec<PathBuf> {
        vec![]
    }
    fn validate(&self) -> Result<()> {
        Ok(())
    }
    fn execute(&self, context: &Context<'_>) -> Result<Self::Output>;
}

trait Erased: Send + Sync {
    fn descriptor(&self) -> Result<serde_json::Value>;
    fn implementation(&self) -> &'static str;
    fn dependencies(&self) -> Vec<Dependency>;
    fn sources(&self) -> Vec<PathBuf>;
    fn execute(&self, context: &Context<'_>) -> Result<Vec<u8>>;
}

impl<T: Operation> Erased for T {
    fn descriptor(&self) -> Result<serde_json::Value> {
        self.validate()?;
        Ok(serde_json::to_value(self)?)
    }
    fn implementation(&self) -> &'static str {
        Operation::implementation(self)
    }
    fn dependencies(&self) -> Vec<Dependency> {
        Operation::dependencies(self)
    }
    fn sources(&self) -> Vec<PathBuf> {
        Operation::sources(self)
    }
    fn execute(&self, context: &Context<'_>) -> Result<Vec<u8>> {
        Ok(postcard::to_stdvec(&Operation::execute(self, context)?)?)
    }
}

struct Job {
    operation: Box<dyn Erased>,
    name: &'static str,
    output: &'static str,
}

pub struct Asset<T> {
    job: Arc<Job>,
    marker: PhantomData<fn() -> T>,
}

impl<T> Clone for Asset<T> {
    fn clone(&self) -> Self {
        Self {
            job: self.job.clone(),
            marker: PhantomData,
        }
    }
}

impl<T> Serialize for Asset<T> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_unit()
    }
}

impl<T: Serialize + DeserializeOwned + Send + Sync + 'static> Asset<T> {
    pub fn new(operation: impl Operation<Output = T>) -> Self {
        Self {
            job: Arc::new(Job {
                name: std::any::type_name_of_val(&operation),
                output: std::any::type_name::<T>(),
                operation: Box::new(operation),
            }),
            marker: PhantomData,
        }
    }
    pub fn dependency(&self) -> Dependency {
        Dependency(self.job.clone())
    }
}

#[derive(Clone)]
pub struct Dependency(Arc<Job>);

pub trait Artifact {
    fn bytes(&self) -> &[u8];
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Bytes(pub Vec<u8>);
impl Artifact for Bytes {
    fn bytes(&self) -> &[u8] {
        &self.0
    }
}

pub struct Published<T> {
    pub asset: Asset<T>,
    pub path: PathBuf,
}
impl<T> Clone for Published<T> {
    fn clone(&self) -> Self {
        Self {
            asset: self.asset.clone(),
            path: self.path.clone(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Record {
    spec: String,
    sources: BTreeMap<PathBuf, String>,
    content: String,
    value: String,
    provenance: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct Root {
    spec: String,
    value: String,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    records: BTreeMap<String, Record>,
    roots: BTreeMap<PathBuf, Root>,
    files: BTreeMap<PathBuf, String>,
}

type Encode = dyn Fn(&[u8]) -> Result<Vec<u8>>;
struct Export {
    path: PathBuf,
    job: Arc<Job>,
    encode: Box<Encode>,
}

#[derive(Default, Debug)]
pub struct Report {
    pub executed: usize,
    pub cached: usize,
    pub written: usize,
    pub operations: Vec<String>,
}

pub struct Pipeline {
    root: PathBuf,
    published: PathBuf,
    cache: PathBuf,
    exports: Vec<Export>,
    lockfile: lockfile::Lockfile,
}

pub struct LegacyPackage {
    provider: String,
    implementation: String,
}

impl LegacyPackage {
    pub fn from_manifest(directory: impl AsRef<Path>, previous_manifest: &[u8]) -> Result<Self> {
        let directory = directory.as_ref();
        let manifest: toml::Value =
            toml::from_str(&fs::read_to_string(directory.join("Cargo.toml"))?)?;
        let previous: toml::Value = toml::from_str(std::str::from_utf8(previous_manifest)?)?;
        let mut normalized = manifest.clone();
        normalize_git_revisions(&mut normalized, &previous);
        if normalized != previous {
            return Err(invalid(
                "legacy package migration permits only git revision changes",
            ));
        }
        let name = manifest
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
            .ok_or_else(|| invalid("missing provider package name"))?;
        Ok(Self {
            provider: name.replace('-', "_"),
            implementation: fingerprint::package_fingerprint(
                directory,
                &[],
                Some(previous_manifest),
                false,
            )?,
        })
    }
}

fn normalize_git_revisions(current: &mut toml::Value, previous: &toml::Value) {
    for section in ["dependencies", "build-dependencies", "dev-dependencies"] {
        if let Some(current) = current.get_mut(section).and_then(toml::Value::as_table_mut)
            && let Some(previous) = previous.get(section).and_then(toml::Value::as_table)
        {
            for (name, dependency) in current {
                if let Some(previous) = previous.get(name)
                    && dependency
                        .get("git")
                        .and_then(toml::Value::as_str)
                        .is_some()
                    && dependency.get("git") == previous.get("git")
                    && let Some(revision) = previous.get("rev")
                    && dependency.get("rev").is_some()
                {
                    dependency["rev"] = revision.clone();
                }
            }
        }
    }
    if let Some(current) = current
        .get_mut("target")
        .and_then(toml::Value::as_table_mut)
        && let Some(previous) = previous.get("target").and_then(toml::Value::as_table)
    {
        for (target, current) in current {
            if let Some(previous) = previous.get(target) {
                normalize_git_revisions(current, previous);
            }
        }
    }
}

impl Pipeline {
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        let lock = root
            .ancestors()
            .map(|p| p.join("Cargo.lock"))
            .find(|p| p.is_file())
            .ok_or_else(|| invalid("asset projects require a Cargo.lock"))?;
        let lockfile = lockfile::Lockfile::parse(&fs::read(&lock)?)?;
        Ok(Self {
            published: root.join("assets/generated"),
            cache: root.join("target/inslag-assets"),
            root,
            exports: vec![],
            lockfile,
        })
    }
    pub fn publish_to(mut self, path: impl AsRef<Path>) -> Result<Self> {
        safe_relative(path.as_ref())?;
        self.published = self.root.join(path);
        Ok(self)
    }
    pub fn export<T>(&mut self, path: impl AsRef<Path>, asset: Asset<T>) -> Result<Published<T>>
    where
        T: Artifact + Serialize + DeserializeOwned + Send + Sync + 'static,
    {
        let path = path.as_ref().to_path_buf();
        safe_relative(&path)?;
        if path.components().any(|part| {
            part.as_os_str()
                .to_string_lossy()
                .starts_with(".inslag-assets")
        }) {
            return Err(invalid("asset path uses a reserved pipeline name"));
        }
        if self
            .exports
            .iter()
            .any(|old| path.starts_with(&old.path) || old.path.starts_with(&path))
        {
            return Err(invalid(format!("conflicting export {}", path.display())));
        }
        self.exports.push(Export {
            path: path.clone(),
            job: asset.job.clone(),
            encode: Box::new(|bytes| {
                let artifact: T = postcard::from_bytes(bytes)?;
                Ok(artifact.bytes().to_vec())
            }),
        });
        Ok(Published { asset, path })
    }
    pub fn run(self) -> Result<()> {
        let arguments: Vec<_> = std::env::args().skip(1).collect();
        match arguments.as_slice() {
            [action] if action == "generate" => {
                let report = self.generate()?;
                eprintln!(
                    "assets: {} executed, {} cached, {} files written",
                    report.executed, report.cached, report.written
                );
                Ok(())
            }
            [action] if action == "check" => self.check(),
            [
                action,
                previous_lock,
                previous_implementation,
                packages @ ..,
            ] if action == "migrate-lock" && packages.len() % 2 == 0 => {
                let packages = packages
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|args| LegacyPackage::from_manifest(&args[0], &fs::read(&args[1])?))
                    .collect::<Result<Vec<_>>>()?;
                self.migrate_lock(
                    &fs::read(previous_lock)?,
                    previous_implementation,
                    &packages,
                )
            }
            _ => Err(invalid(
                "usage: <asset tool> generate | check | migrate-lock <previous Cargo.lock> <previous pipeline implementation> [<package directory> <previous Cargo.toml>]...",
            )),
        }
    }
    pub fn check(self) -> Result<()> {
        reject_symlinks(&self.root, self.published.strip_prefix(&self.root)?)?;
        reject_symlinks(&self.published, Path::new(MANIFEST))?;
        let manifest: Manifest =
            serde_json::from_slice(&fs::read(self.published.join(MANIFEST)).map_err(|e| {
                invalid(format!(
                    "missing generated assets: {e}; run the asset tool with generate"
                ))
            })?)?;
        if manifest.version != 2 {
            return Err(invalid(
                "unsupported asset manifest; regenerate assets or migrate its lockfile receipts",
            ));
        }
        let mut evaluator = Evaluator::new(&self, Some(&manifest));
        let mut roots = BTreeMap::new();
        for export in &self.exports {
            let record = evaluator.evaluate(&export.job)?;
            roots.insert(
                export.path.clone(),
                Root {
                    spec: record.spec,
                    value: record.value,
                },
            );
        }
        if roots != manifest.roots || manifest.files.keys().ne(roots.keys()) {
            return Err(invalid("asset exports changed; regenerate assets"));
        }
        for (path, hash) in &manifest.files {
            safe_relative(path)?;
            reject_symlinks(&self.published, path)?;
            if digest_file(&self.published.join(path)).as_ref().ok() != Some(hash) {
                return Err(invalid(format!(
                    "missing or corrupt generated asset {}; regenerate assets",
                    path.display()
                )));
            }
        }
        Ok(())
    }
    pub fn migrate_lock(
        self,
        previous_lock: &[u8],
        previous_implementation: &str,
        packages: &[LegacyPackage],
    ) -> Result<()> {
        fs::create_dir_all(&self.cache)?;
        let _evaluation_lock = lock(&self.cache.join("generate.lock"))?;
        reject_symlinks(&self.root, self.published.strip_prefix(&self.root)?)?;
        reject_symlinks(&self.published, Path::new(MANIFEST))?;
        reject_symlinks(&self.published, Path::new(".inslag-assets.lock"))?;
        let _publication = lock(&self.published.join(".inslag-assets.lock"))?;
        let manifest: Manifest = serde_json::from_slice(&fs::read(self.published.join(MANIFEST))?)?;
        if manifest.version != 1 {
            return Err(invalid("lockfile migration requires a version 1 manifest"));
        }
        let mut providers = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut pending = self
            .exports
            .iter()
            .map(|export| export.job.clone())
            .collect::<Vec<_>>();
        while let Some(job) = pending.pop() {
            if seen.insert(Arc::as_ptr(&job) as usize) {
                providers.insert(
                    job.name
                        .split("::")
                        .next()
                        .unwrap_or(job.name)
                        .replace('-', "_"),
                );
                pending.extend(
                    job.operation
                        .dependencies()
                        .into_iter()
                        .map(|dependency| dependency.0),
                );
            }
        }
        self.lockfile
            .validate_migration(&lockfile::Lockfile::parse(previous_lock)?, &providers)?;
        let mut evaluator = Evaluator::new(&self, None);
        evaluator.legacy = Some(Legacy {
            manifest: &manifest,
            implementation: previous_implementation,
            lockfile: digest(previous_lock),
            packages: packages
                .iter()
                .map(|package| (package.provider.as_str(), package.implementation.as_str()))
                .collect(),
        });
        let mut previous_roots = BTreeMap::new();
        let mut roots = BTreeMap::new();
        for export in &self.exports {
            let record = evaluator.evaluate(&export.job)?;
            previous_roots.insert(
                export.path.clone(),
                Root {
                    spec: evaluator.previous_specs[&(Arc::as_ptr(&export.job) as usize)].clone(),
                    value: record.value.clone(),
                },
            );
            roots.insert(
                export.path.clone(),
                Root {
                    spec: record.spec,
                    value: record.value,
                },
            );
        }
        if previous_roots != manifest.roots || manifest.files.keys().ne(roots.keys()) {
            return Err(invalid("asset exports changed; regeneration is required"));
        }
        for (path, hash) in &manifest.files {
            safe_relative(path)?;
            reject_symlinks(&self.published, path)?;
            if digest_file(&self.published.join(path)).as_ref().ok() != Some(hash) {
                return Err(invalid(format!(
                    "missing or corrupt generated asset {}; regeneration is required",
                    path.display()
                )));
            }
        }
        for (id, record) in &evaluator.memo {
            let prior = &manifest.records[&evaluator.previous_specs[id]];
            let old_value = value_path(&self.cache, &prior.content);
            if fs::symlink_metadata(&old_value).is_ok_and(|metadata| metadata.is_file())
                && digest_file(&old_value).as_ref().ok() == Some(&record.value)
            {
                let new_value = value_path(&self.cache, &record.content);
                if !new_value.exists() {
                    fs::create_dir_all(new_value.parent().unwrap())?;
                    fs::hard_link(&old_value, &new_value)?;
                }
                if digest_file(&new_value).as_ref().ok() != Some(&record.value) {
                    return Err(invalid("artifact cache changed during migration"));
                }
                let bytes = serde_json::to_vec(record)?;
                write_changed(&new_value.with_extension("json"), &bytes)?;
                write_changed(
                    &self
                        .cache
                        .join("specs")
                        .join(format!("{}.json", record.spec)),
                    &bytes,
                )?;
            }
        }
        let upgraded = Manifest {
            version: 2,
            records: evaluator.records,
            roots,
            files: manifest.files.clone(),
        };
        write_changed(
            &self.published.join(MANIFEST),
            &serde_json::to_vec_pretty(&upgraded)?,
        )?;
        Ok(())
    }
    pub fn generate(self) -> Result<Report> {
        fs::create_dir_all(&self.cache)?;
        let evaluation_lock = lock(&self.cache.join("generate.lock"))?;
        let mut evaluator = Evaluator::new(&self, None);
        let mut files = BTreeMap::new();
        let mut roots = BTreeMap::new();
        for export in &self.exports {
            let record = evaluator.evaluate(&export.job)?;
            let bytes = fs::read(value_path(&self.cache, &record.content))?;
            if digest(&bytes) != record.value {
                return Err(invalid("artifact cache changed during publication"));
            }
            files.insert(export.path.clone(), (export.encode)(&bytes)?);
            roots.insert(
                export.path.clone(),
                Root {
                    spec: record.spec,
                    value: record.value,
                },
            );
        }
        reject_symlinks(&self.root, self.published.strip_prefix(&self.root)?)?;
        fs::create_dir_all(&self.published)?;
        reject_symlinks(&self.published, Path::new(".inslag-assets.lock"))?;
        reject_symlinks(&self.published, Path::new(MANIFEST))?;
        let publication = lock(&self.published.join(".inslag-assets.lock"))?;
        let old: Option<Manifest> = match fs::read(self.published.join(MANIFEST)) {
            Ok(bytes) => Some(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        for path in files.keys().chain(old.iter().flat_map(|m| m.files.keys())) {
            safe_relative(path)?;
            reject_symlinks(&self.published, path)?;
        }
        for path in files.keys() {
            if self.published.join(path).exists()
                && !old.as_ref().is_some_and(|m| m.files.contains_key(path))
            {
                return Err(invalid(format!(
                    "refusing to overwrite unmanaged asset {}",
                    path.display()
                )));
            }
        }
        let manifest = Manifest {
            version: 2,
            records: evaluator.records.clone(),
            roots,
            files: files.iter().map(|(p, b)| (p.clone(), digest(b))).collect(),
        };
        for (path, bytes) in files {
            evaluator.report.written +=
                usize::from(write_changed(&self.published.join(path), &bytes)?);
        }
        if let Some(old) = old {
            for path in old
                .files
                .keys()
                .filter(|path| !manifest.files.contains_key(*path))
            {
                let path = self.published.join(path);
                if path.is_file() {
                    fs::remove_file(path)?;
                }
            }
        }
        write_changed(
            &self.published.join(MANIFEST),
            &serde_json::to_vec_pretty(&manifest)?,
        )?;
        drop(publication);
        drop(evaluation_lock);
        Ok(evaluator.report)
    }
}

pub struct Context<'a> {
    root: &'a Path,
    cache: &'a Path,
    inputs: &'a BTreeMap<usize, Record>,
    allowed_inputs: BTreeSet<usize>,
    decoded: RefCell<BTreeMap<usize, Arc<dyn Any + Send + Sync>>>,
    tracked: RefCell<BTreeMap<PathBuf, String>>,
    provenance: RefCell<BTreeMap<String, String>>,
    temporary: tempfile::TempDir,
}

impl<'a> Context<'a> {
    fn new(
        root: &'a Path,
        cache: &'a Path,
        inputs: &'a BTreeMap<usize, Record>,
        allowed_inputs: BTreeSet<usize>,
        temporary: tempfile::TempDir,
    ) -> Self {
        Self {
            root,
            cache,
            inputs,
            allowed_inputs,
            decoded: RefCell::new(BTreeMap::new()),
            tracked: RefCell::new(BTreeMap::new()),
            provenance: RefCell::new(BTreeMap::new()),
            temporary,
        }
    }
    pub fn root(&self) -> &Path {
        self.root
    }
    pub fn work_dir(&self) -> &Path {
        self.temporary.path()
    }
    pub fn record_provenance(&self, name: impl Into<String>, value: impl Into<String>) {
        self.provenance
            .borrow_mut()
            .insert(name.into(), value.into());
    }
    pub fn source(&self, path: impl AsRef<Path>) -> Result<PathBuf> {
        let absolute = self.root.join(path).canonicalize()?;
        let relative = absolute
            .strip_prefix(self.root)
            .map_err(|_| invalid(format!("source outside project: {}", absolute.display())))?
            .to_path_buf();
        let digest = digest_file(&absolute)?;
        if self
            .tracked
            .borrow_mut()
            .insert(relative, digest.clone())
            .is_some_and(|old| old != digest)
        {
            return Err(invalid("source changed during processor execution"));
        }
        Ok(absolute)
    }
    pub fn input<T: DeserializeOwned + Send + Sync + 'static>(
        &self,
        asset: &Asset<T>,
    ) -> Result<Arc<T>> {
        let id = Arc::as_ptr(&asset.job) as usize;
        if !self.allowed_inputs.contains(&id) {
            return Err(invalid("processor accessed an undeclared dependency"));
        }
        if let Some(value) = self.decoded.borrow().get(&id) {
            return value
                .clone()
                .downcast()
                .map_err(|_| invalid("processor input type mismatch"));
        }
        let record = self
            .inputs
            .get(&id)
            .ok_or_else(|| invalid("processor accessed an undeclared dependency"))?;
        let bytes = fs::read(value_path(self.cache, &record.content))?;
        if digest(&bytes) != record.value {
            return Err(invalid("processor input cache changed during evaluation"));
        }
        let value: Arc<T> = Arc::new(postcard::from_bytes(&bytes)?);
        self.decoded.borrow_mut().insert(id, value.clone());
        Ok(value)
    }
}

struct Evaluator<'a> {
    pipeline: &'a Pipeline,
    check: Option<&'a Manifest>,
    memo: BTreeMap<usize, Record>,
    records: BTreeMap<String, Record>,
    report: Report,
    lockfiles: BTreeMap<String, String>,
    legacy: Option<Legacy<'a>>,
    previous_specs: BTreeMap<usize, String>,
    visiting: BTreeSet<usize>,
}

struct Legacy<'a> {
    manifest: &'a Manifest,
    implementation: &'a str,
    lockfile: String,
    packages: BTreeMap<&'a str, &'a str>,
}

impl<'a> Evaluator<'a> {
    fn new(pipeline: &'a Pipeline, check: Option<&'a Manifest>) -> Self {
        Self {
            pipeline,
            check,
            memo: BTreeMap::new(),
            records: BTreeMap::new(),
            report: Report::default(),
            lockfiles: BTreeMap::new(),
            legacy: None,
            previous_specs: BTreeMap::new(),
            visiting: BTreeSet::new(),
        }
    }
    fn evaluate(&mut self, job: &Arc<Job>) -> Result<Record> {
        let id = Arc::as_ptr(job) as usize;
        if let Some(record) = self.memo.get(&id) {
            return Ok(record.clone());
        }
        if !self.visiting.insert(id) {
            return Err(invalid("asset dependency cycle"));
        }
        let descriptor = job.operation.descriptor()?;
        let inputs = job.operation.dependencies();
        let dependencies = inputs
            .iter()
            .map(|d| self.evaluate(&d.0).map(|r| r.value))
            .collect::<Result<Vec<_>>>()?;
        let provider = job.name.split("::").next().unwrap_or(job.name);
        if !self.lockfiles.contains_key(provider) {
            self.lockfiles.insert(
                provider.into(),
                self.pipeline.lockfile.fingerprint(job.name)?,
            );
        }
        let spec = digest(&serde_json::to_vec(&(
            IMPLEMENTATION,
            &self.lockfiles[provider],
            job.name,
            job.output,
            job.operation.implementation(),
            &descriptor,
            &dependencies,
        ))?);
        if let Some(legacy) = &self.legacy {
            let implementation = legacy
                .packages
                .get(provider)
                .copied()
                .unwrap_or_else(|| job.operation.implementation());
            let previous_spec = digest(&serde_json::to_vec(&(
                legacy.implementation,
                &legacy.lockfile,
                job.name,
                job.output,
                implementation,
                &descriptor,
                &dependencies,
            ))?);
            let prior = legacy.manifest.records.get(&previous_spec).ok_or_else(|| {
                invalid(format!(
                    "{} implementation or parameters changed; regeneration is required",
                    job.name
                ))
            })?;
            let mut paths: BTreeSet<_> = job.operation.sources().into_iter().collect();
            paths.extend(prior.sources.keys().cloned());
            let sources = hash_sources(&self.pipeline.root, &paths)?;
            if sources != prior.sources
                || prior.content != digest(&serde_json::to_vec(&(&previous_spec, &sources))?)
            {
                return Err(invalid(format!(
                    "{} sources changed; regeneration is required",
                    job.name
                )));
            }
            let record = Record {
                spec: spec.clone(),
                sources,
                content: digest(&serde_json::to_vec(&(&spec, &prior.sources))?),
                value: prior.value.clone(),
                provenance: prior.provenance.clone(),
            };
            self.previous_specs.insert(id, previous_spec);
            self.records.insert(spec, record.clone());
            self.memo.insert(id, record.clone());
            self.visiting.remove(&id);
            return Ok(record);
        }
        let index = self
            .pipeline
            .cache
            .join("specs")
            .join(format!("{spec}.json"));
        let prior =
            if let Some(manifest) = self.check {
                Some(
                    manifest.records.get(&spec).cloned().ok_or_else(|| {
                        invalid(format!("{} changed; regenerate assets", job.name))
                    })?,
                )
            } else {
                fs::read(&index)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Record>(&bytes).ok())
            };
        let declared: BTreeSet<_> = job.operation.sources().into_iter().collect();
        let declared_sources = hash_sources(&self.pipeline.root, &declared)?;
        let mut paths = declared.clone();
        if let Some(prior) = &prior {
            paths.extend(
                prior
                    .sources
                    .keys()
                    .filter(|p| self.check.is_some() || self.pipeline.root.join(p).exists())
                    .cloned(),
            );
        }
        let sources = hash_sources(&self.pipeline.root, &paths)?;
        let content = digest(&serde_json::to_vec(&(&spec, &sources))?);
        let file = value_path(&self.pipeline.cache, &content);
        let metadata = file.with_extension("json");
        let record = if let Some(manifest) = self.check {
            let record = &manifest.records[&spec];
            if record.sources != sources || record.content != content {
                return Err(invalid(format!(
                    "{} sources changed; regenerate assets",
                    job.name
                )));
            }
            record.clone()
        } else if let Some(record) = fs::read(&metadata)
            .ok()
            .and_then(|b| serde_json::from_slice::<Record>(&b).ok())
            .filter(|r| {
                r.spec == spec
                    && r.sources == sources
                    && r.content == content
                    && digest_file(&file).as_ref().ok() == Some(&r.value)
            })
        {
            self.report.cached += 1;
            record
        } else {
            fs::create_dir_all(&self.pipeline.cache)?;
            let ctx = Context::new(
                &self.pipeline.root,
                &self.pipeline.cache,
                &self.memo,
                inputs.iter().map(|d| Arc::as_ptr(&d.0) as usize).collect(),
                tempfile::tempdir_in(&self.pipeline.cache)?,
            );
            let bytes = job
                .operation
                .execute(&ctx)
                .map_err(|e| invalid(format!("{}: {e}", job.name)))?;
            let tracked = ctx.tracked.into_inner();
            paths = declared;
            paths.extend(tracked.keys().cloned());
            let provenance = ctx.provenance.into_inner();
            let sources_after = hash_sources(&self.pipeline.root, &paths)?;
            for (path, before) in declared_sources.iter().chain(tracked.iter()) {
                if sources_after.get(path) != Some(before) {
                    return Err(invalid(format!(
                        "{} changed during processing",
                        path.display()
                    )));
                }
            }
            let content = digest(&serde_json::to_vec(&(&spec, &sources_after))?);
            let record = Record {
                spec: spec.clone(),
                sources: sources_after,
                content,
                value: digest(&bytes),
                provenance,
            };
            let file = value_path(&self.pipeline.cache, &record.content);
            write_changed(&file, &bytes)?;
            write_changed(&file.with_extension("json"), &serde_json::to_vec(&record)?)?;
            self.report.executed += 1;
            self.report.operations.push(job.name.into());
            record
        };
        if self.check.is_none() {
            write_changed(&index, &serde_json::to_vec(&record)?)?;
        }
        self.records.insert(spec, record.clone());
        self.memo.insert(id, record.clone());
        self.visiting.remove(&id);
        Ok(record)
    }
}

fn value_path(cache: &Path, key: &str) -> PathBuf {
    cache.join("values").join(key).join("value.bin")
}
fn hash_sources(root: &Path, paths: &BTreeSet<PathBuf>) -> Result<BTreeMap<PathBuf, String>> {
    paths
        .iter()
        .map(|path| {
            safe_relative(path)?;
            let absolute = root.join(path).canonicalize()?;
            if !absolute.starts_with(root) {
                return Err(invalid("source symlink escapes project"));
            }
            Ok((path.clone(), digest_file(&absolute)?))
        })
        .collect()
}
fn safe_relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(invalid(format!(
            "expected a nonempty relative asset path: {}",
            path.display()
        )));
    }
    Ok(())
}
fn reject_symlinks(root: &Path, path: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    for part in path.components() {
        current.push(part);
        if fs::symlink_metadata(&current).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(invalid(format!(
                "asset path traverses symlink {}",
                current.display()
            )));
        }
    }
    Ok(())
}
fn lock(path: &Path) -> Result<fs::File> {
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    file.lock()?;
    Ok(file)
}
fn write_changed(path: &Path, bytes: &[u8]) -> Result<bool> {
    if fs::read(path).ok().as_deref() == Some(bytes) {
        return Ok(false);
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("output needs a parent"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(true)
}

pub fn invalid(message: impl Into<String>) -> Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into()).into()
}

fn digest_file(path: &Path) -> Result<String> {
    Ok(digest(&fs::read(path)?))
}

fn digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
