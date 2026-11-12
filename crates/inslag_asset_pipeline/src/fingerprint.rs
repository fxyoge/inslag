pub fn fingerprint_package() {
    fingerprint_package_with_dependencies(&[]);
}

pub fn fingerprint_package_with_dependencies(dependencies: &[&str]) {
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    println!(
        "cargo::rustc-env=INSLAG_ASSET_IMPLEMENTATION={}",
        package_fingerprint(&root, dependencies, None, true).unwrap()
    );
}

pub(crate) fn package_fingerprint(
    root: &std::path::Path,
    dependencies: &[&str],
    previous_manifest: Option<&[u8]>,
    announce: bool,
) -> std::io::Result<String> {
    fn visit(
        root: &std::path::Path,
        path: &std::path::Path,
        hash: &mut blake3::Hasher,
        previous_manifest: Option<&[u8]>,
        announce: bool,
    ) -> std::io::Result<()> {
        if announce {
            println!("cargo::rerun-if-changed={}", path.display());
        }
        if path.is_dir() {
            let mut entries = std::fs::read_dir(path)?
                .map(|entry| entry.map(|entry| entry.path()))
                .collect::<std::io::Result<Vec<_>>>()?;
            entries.sort();
            for entry in entries {
                visit(root, &entry, hash, previous_manifest, announce)?;
            }
        } else {
            hash.update(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .as_bytes(),
            );
            hash.update(&[0]);
            if path == root.join("Cargo.toml")
                && let Some(manifest) = previous_manifest
            {
                hash.update(manifest);
            } else {
                hash.update(&std::fs::read(path)?);
            }
            hash.update(&[0]);
        }
        Ok(())
    }
    let mut hash = blake3::Hasher::new();
    for dependency in dependencies {
        hash.update(dependency.as_bytes()).update(&[0]);
    }
    for name in ["src", "Cargo.toml", "build.rs"] {
        let path = root.join(name);
        if path.exists() {
            visit(root, &path, &mut hash, previous_manifest, announce)?;
        }
    }
    Ok(hash.finalize().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_only_migrations_cannot_hide_changed_processor_code() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/lib.rs"), "original processor").unwrap();
        let manifest = b"[package]\nname='processor'\nversion='0.1.0'\n";
        std::fs::write(root.path().join("Cargo.toml"), manifest).unwrap();
        let before = package_fingerprint(root.path(), &[], None, false).unwrap();
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='processor'\nversion='0.1.0'\n[dependencies]\ncodec='1'\n",
        )
        .unwrap();
        assert_eq!(
            before,
            package_fingerprint(root.path(), &[], Some(manifest), false).unwrap()
        );
        std::fs::write(root.path().join("src/lib.rs"), "changed processor").unwrap();
        assert_ne!(
            before,
            package_fingerprint(root.path(), &[], Some(manifest), false).unwrap()
        );
    }
}
