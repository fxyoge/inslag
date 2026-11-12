use inslag_asset_pipeline::*;
use serde::Serialize;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const LOCK: &str = "version=4\n[[package]]\nname='pipeline'\nversion='0.1.0'\ndependencies=['codec']\n[[package]]\nname='codec'\nversion='1.0.0'\n";

#[derive(Serialize)]
struct Source {
    value: String,
    file: Option<PathBuf>,
    extra: Option<PathBuf>,
    fail: bool,
    #[serde(skip)]
    calls: Arc<AtomicUsize>,
}
impl Operation for Source {
    type Output = Bytes;
    fn implementation(&self) -> &'static str {
        "test-source"
    }
    fn sources(&self) -> Vec<PathBuf> {
        self.file.iter().cloned().collect()
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<Bytes> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            return Err(invalid("deliberate failure"));
        }
        let mut value = self.value.as_bytes().to_vec();
        if let Some(path) = &self.file {
            value.extend(fs::read(ctx.source(path)?)?);
        }
        if let Some(path) = &self.extra {
            value.extend(fs::read(ctx.source(path)?)?);
        }
        Ok(Bytes(value))
    }
}
#[derive(Serialize)]
struct Same {
    input: Asset<Bytes>,
    setting: u32,
}
impl Operation for Same {
    type Output = Bytes;
    fn implementation(&self) -> &'static str {
        "same"
    }
    fn dependencies(&self) -> Vec<Dependency> {
        vec![self.input.dependency()]
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<Bytes> {
        let _ = ctx.input(&self.input)?;
        Ok(Bytes(b"same".to_vec()))
    }
}

fn source(value: &str, calls: &Arc<AtomicUsize>) -> Asset<Bytes> {
    Asset::new(Source {
        value: value.into(),
        file: None,
        extra: None,
        fail: false,
        calls: calls.clone(),
    })
}

#[test]
fn lazy_warm_cache_check_without_cache_and_missing_output_recovery() -> Result<()> {
    let root = project()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let _unused = source("unused", &calls);
    let pipeline = || -> Result<Pipeline> {
        let mut p = Pipeline::new(root.path())?;
        p.export("one.txt", source("one", &calls))?;
        Ok(p)
    };
    assert_eq!(pipeline()?.generate()?.executed, 1);
    let file = root.path().join("assets/generated/one.txt");
    let before = fs::metadata(&file)?.modified()?;
    assert_eq!(pipeline()?.generate()?.executed, 0);
    assert_eq!(before, fs::metadata(&file)?.modified()?);
    fs::remove_file(&file)?;
    assert!(pipeline()?.check().is_err());
    assert_eq!(pipeline()?.generate()?.executed, 0);
    assert!(file.exists());
    fs::remove_dir_all(root.path().join("target"))?;
    pipeline()?.check()?;
    assert!(!root.path().join("target").exists());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn declared_and_discovered_sources_invalidate_and_reverts_reuse() -> Result<()> {
    let root = project()?;
    fs::write(root.path().join("input"), "a")?;
    fs::write(root.path().join("linked"), "b")?;
    let calls = Arc::new(AtomicUsize::new(0));
    let pipeline = || -> Result<Pipeline> {
        let mut p = Pipeline::new(root.path())?;
        p.export(
            "result",
            Asset::new(Source {
                value: String::new(),
                file: Some("input".into()),
                extra: Some("linked".into()),
                fail: false,
                calls: calls.clone(),
            }),
        )?;
        Ok(p)
    };
    assert_eq!(pipeline()?.generate()?.executed, 1);
    fs::write(root.path().join("linked"), "c")?;
    assert!(pipeline()?.check().is_err());
    assert_eq!(pipeline()?.generate()?.executed, 1);
    fs::write(root.path().join("linked"), "b")?;
    assert_eq!(pipeline()?.generate()?.executed, 0);
    fs::write(root.path().join("input"), "d")?;
    assert!(pipeline()?.check().is_err());
    assert_eq!(pipeline()?.generate()?.executed, 1);
    Ok(())
}

#[test]
fn identical_results_stop_downstream_invalidation_and_exports_are_strict() -> Result<()> {
    let root = project()?;
    let calls = Arc::new(AtomicUsize::new(0));
    for (value, expected) in [("a", 3), ("b", 2)] {
        let source = source(value, &calls);
        let same = Asset::new(Same {
            input: source,
            setting: 1,
        });
        let end = Asset::new(Same {
            input: same,
            setting: 2,
        });
        let mut p = Pipeline::new(root.path())?;
        p.export("result", end)?;
        assert_eq!(p.generate()?.executed, expected);
    }
    let mut p = Pipeline::new(root.path())?;
    p.export("different", source("a", &calls))?;
    assert!(p.check().is_err());
    Ok(())
}

#[test]
fn failed_generation_preserves_published_assets_and_unmanaged_files() -> Result<()> {
    let root = project()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut p = Pipeline::new(root.path())?;
    p.export("good", source("original", &calls))?;
    p.generate()?;
    let dir = root.path().join("assets/generated");
    fs::write(dir.join("authored"), "hands off")?;
    let mut p = Pipeline::new(root.path())?;
    p.export("good", source("changed", &calls))?;
    p.export(
        "failure",
        Asset::new(Source {
            value: String::new(),
            file: None,
            extra: None,
            fail: true,
            calls: calls.clone(),
        }),
    )?;
    assert!(p.generate().is_err());
    assert_eq!(fs::read_to_string(dir.join("good"))?, "original");
    let mut p = Pipeline::new(root.path())?;
    p.export("authored", source("replacement", &calls))?;
    assert!(p.generate().is_err());
    Pipeline::new(root.path())?.generate()?;
    assert!(!dir.join("good").exists());
    assert_eq!(fs::read_to_string(dir.join("authored"))?, "hands off");
    Ok(())
}

#[test]
fn unsafe_duplicate_and_prefix_paths_fail() -> Result<()> {
    let root = project()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut p = Pipeline::new(root.path())?;
    for path in ["", "../outside", "/absolute", ".inslag-assets.json"] {
        assert!(p.export(path, source("x", &calls)).is_err());
    }
    p.export("one", source("x", &calls))?;
    assert!(p.export("one", source("x", &calls)).is_err());
    assert!(p.export("one/two", source("x", &calls)).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn publication_refuses_root_and_nested_symlinks() -> Result<()> {
    let root = project()?;
    fs::create_dir_all(root.path().join("assets"))?;
    let elsewhere = tempfile::tempdir()?;
    std::os::unix::fs::symlink(elsewhere.path(), root.path().join("assets/generated"))?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut p = Pipeline::new(root.path())?;
    p.export("result", source("x", &calls))?;
    assert!(p.generate().is_err());
    assert!(!elsewhere.path().join("result").exists());
    Ok(())
}

#[derive(Serialize)]
struct Discover;
impl Operation for Discover {
    type Output = Bytes;
    fn implementation(&self) -> &'static str {
        "discover"
    }
    fn sources(&self) -> Vec<PathBuf> {
        vec!["input".into()]
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<Bytes> {
        let name = fs::read_to_string(ctx.source("input")?)?;
        Ok(Bytes(if name.is_empty() {
            vec![]
        } else {
            fs::read(ctx.source(name)?)?
        }))
    }
}

#[test]
fn removed_discovered_inputs_can_be_regenerated_and_are_not_retained() -> Result<()> {
    let root = project()?;
    fs::write(root.path().join("input"), "linked")?;
    fs::write(root.path().join("linked"), "data")?;
    let pipeline = || -> Result<Pipeline> {
        let mut p = Pipeline::new(root.path())?;
        p.export("result", Asset::new(Discover))?;
        Ok(p)
    };
    pipeline()?.generate()?;
    fs::write(root.path().join("input"), "")?;
    fs::remove_file(root.path().join("linked"))?;
    assert!(pipeline()?.check().is_err());
    assert_eq!(pipeline()?.generate()?.executed, 1);
    pipeline()?.check()?;
    assert_eq!(pipeline()?.generate()?.executed, 0);
    Ok(())
}

#[derive(Serialize)]
struct Undeclared {
    input: Asset<Bytes>,
}
impl Operation for Undeclared {
    type Output = Bytes;
    fn implementation(&self) -> &'static str {
        "undeclared"
    }
    fn execute(&self, ctx: &Context<'_>) -> Result<Bytes> {
        Ok(Bytes(ctx.input(&self.input)?.0.clone()))
    }
}

#[test]
fn even_already_evaluated_inputs_must_be_declared() -> Result<()> {
    let root = project()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let input = source("first", &calls);
    let mut p = Pipeline::new(root.path())?;
    p.export("first", input.clone())?;
    p.export("undeclared", Asset::new(Undeclared { input }))?;
    assert!(
        p.generate()
            .unwrap_err()
            .to_string()
            .contains("undeclared dependency")
    );
    Ok(())
}

#[test]
fn corrupt_cache_is_repaired_and_lock_changes_invalidate_verification() -> Result<()> {
    let root = project()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let pipeline = || -> Result<Pipeline> {
        let mut p = Pipeline::new(root.path())?;
        p.export("result", source("data", &calls))?;
        Ok(p)
    };
    pipeline()?.generate()?;
    let values = root.path().join("target/inslag-assets/values");
    let entry = fs::read_dir(values)?
        .next()
        .unwrap()?
        .path()
        .join("value.bin");
    fs::write(entry, "corrupt")?;
    pipeline()?.check()?;
    assert_eq!(pipeline()?.generate()?.executed, 1);
    fs::write(
        root.path().join("Cargo.lock"),
        LOCK.replace("version='1.0.0'", "version='2.0.0'"),
    )?;
    assert!(pipeline()?.check().is_err());
    assert_eq!(pipeline()?.generate()?.executed, 1);
    Ok(())
}

#[test]
fn unrelated_workspace_dependencies_do_not_invalidate_generated_assets() -> Result<()> {
    let root = project()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let pipeline = || -> Result<Pipeline> {
        let mut pipeline = Pipeline::new(root.path())?;
        pipeline.export("result", source("data", &calls))?;
        Ok(pipeline)
    };
    pipeline()?.generate()?;
    fs::write(
        root.path().join("Cargo.lock"),
        format!(
            "{LOCK}\n[[package]]\nname='game'\nversion='0.1.0'\ndependencies=['test-helper']\n[[package]]\nname='test-helper'\nversion='3.0.0'\n"
        ),
    )?;
    pipeline()?.check()?;
    assert_eq!(pipeline()?.generate()?.executed, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

fn legacy_project() -> Result<(tempfile::TempDir, Arc<AtomicUsize>)> {
    let root = project()?;
    let calls = Arc::new(AtomicUsize::new(0));
    let operation = Source {
        value: "data".into(),
        file: Some("input".into()),
        extra: None,
        fail: false,
        calls: calls.clone(),
    };
    fs::write(root.path().join("input"), "source")?;
    let descriptor = serde_json::to_value(&operation)?;
    let mut pipeline = Pipeline::new(root.path())?;
    pipeline.export("result", Asset::new(operation))?;
    pipeline.generate()?;
    let path = root.path().join("assets/generated/.inslag-assets.json");
    let mut manifest: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    let mut record = manifest["records"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    let old_spec = blake3::hash(&serde_json::to_vec(&(
        "legacy-pipeline",
        blake3::hash(LOCK.as_bytes()).to_string(),
        std::any::type_name::<Source>(),
        std::any::type_name::<Bytes>(),
        "test-source",
        descriptor,
        Vec::<String>::new(),
    ))?)
    .to_string();
    let old_content =
        blake3::hash(&serde_json::to_vec(&(&old_spec, &record["sources"]))?).to_string();
    let cache = root.path().join("target/inslag-assets");
    let current_content = record["content"].as_str().unwrap().to_string();
    let current_spec = record["spec"].as_str().unwrap().to_string();
    fs::create_dir_all(cache.join("values").join(&old_content))?;
    fs::copy(
        cache
            .join("values")
            .join(&current_content)
            .join("value.bin"),
        cache.join("values").join(&old_content).join("value.bin"),
    )?;
    fs::remove_dir_all(cache.join("values").join(&current_content))?;
    fs::remove_file(cache.join("specs").join(format!("{current_spec}.json")))?;
    record["spec"] = old_spec.clone().into();
    record["content"] = old_content.into();
    manifest["version"] = 1.into();
    manifest["records"] = serde_json::json!({old_spec.clone(): record});
    manifest["roots"]["result"]["spec"] = old_spec.into();
    fs::write(path, serde_json::to_vec_pretty(&manifest)?)?;
    Ok((root, calls))
}

fn legacy_pipeline(root: &std::path::Path, calls: &Arc<AtomicUsize>) -> Result<Pipeline> {
    let mut pipeline = Pipeline::new(root)?;
    pipeline.export(
        "result",
        Asset::new(Source {
            value: "data".into(),
            file: Some("input".into()),
            extra: None,
            fail: false,
            calls: calls.clone(),
        }),
    )?;
    Ok(pipeline)
}

#[test]
fn legacy_migration_preserves_pixels_and_cached_results_without_executing_processors() -> Result<()>
{
    for cache in [false, true] {
        let (root, calls) = legacy_project()?;
        let output = root.path().join("assets/generated/result");
        let bytes = fs::read(&output)?;
        let modified = fs::metadata(&output)?.modified()?;
        if !cache {
            fs::remove_dir_all(root.path().join("target"))?;
        }
        assert!(legacy_pipeline(root.path(), &calls)?.check().is_err());
        legacy_pipeline(root.path(), &calls)?.migrate_lock(
            LOCK.as_bytes(),
            "legacy-pipeline",
            &[],
        )?;
        legacy_pipeline(root.path(), &calls)?.check()?;
        assert_eq!(fs::read(&output)?, bytes);
        assert_eq!(fs::metadata(&output)?.modified()?, modified);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        if cache {
            assert_eq!(
                legacy_pipeline(root.path(), &calls)?.generate()?.executed,
                0
            );
        }
    }
    Ok(())
}

#[test]
fn legacy_migration_rejects_changed_inputs_dependencies_outputs_and_parameters_atomically()
-> Result<()> {
    for change in ["input", "dependency", "output", "parameter"] {
        let (root, calls) = legacy_project()?;
        let path = root.path().join("assets/generated/.inslag-assets.json");
        let before = fs::read(&path)?;
        match change {
            "input" => fs::write(root.path().join("input"), "changed")?,
            "dependency" => fs::write(
                root.path().join("Cargo.lock"),
                LOCK.replace("version='1.0.0'", "version='2.0.0'"),
            )?,
            "output" => fs::write(root.path().join("assets/generated/result"), "corrupt")?,
            _ => {}
        }
        let pipeline = if change == "parameter" {
            let mut pipeline = Pipeline::new(root.path())?;
            pipeline.export("result", source("changed", &calls))?;
            pipeline
        } else {
            legacy_pipeline(root.path(), &calls)?
        };
        assert!(
            pipeline
                .migrate_lock(LOCK.as_bytes(), "legacy-pipeline", &[])
                .is_err(),
            "accepted changed {change}"
        );
        assert_eq!(fs::read(path)?, before);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[test]
fn legacy_manifest_overrides_cannot_hide_dependency_requirement_changes() -> Result<()> {
    let root = tempfile::tempdir()?;
    let previous = "[package]\nname='provider'\nversion='0.1.0'\n[dependencies]\ncodec={git='https://example.invalid/codec',rev='old'}\n";
    fs::write(
        root.path().join("Cargo.toml"),
        previous.replace("rev='old'", "rev='new'"),
    )?;
    assert!(LegacyPackage::from_manifest(root.path(), previous.as_bytes()).is_ok());
    fs::write(
        root.path().join("Cargo.toml"),
        format!("{previous}other='2'\n"),
    )?;
    assert!(LegacyPackage::from_manifest(root.path(), previous.as_bytes()).is_err());
    Ok(())
}

fn project() -> Result<tempfile::TempDir> {
    let root = tempfile::tempdir()?;
    fs::write(root.path().join("Cargo.lock"), LOCK)?;
    Ok(root)
}
