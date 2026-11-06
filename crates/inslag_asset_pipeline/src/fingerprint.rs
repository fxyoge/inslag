pub fn fingerprint_package() {
    fingerprint_package_with_dependencies(&[]);
}

pub fn fingerprint_package_with_dependencies(dependencies: &[&str]) {
    fn visit(root: &std::path::Path, path: &std::path::Path, hash: &mut blake3::Hasher) {
        println!("cargo::rerun-if-changed={}", path.display());
        if path.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(path)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            entries.sort();
            for entry in entries {
                visit(root, &entry, hash);
            }
        } else {
            hash.update(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .as_bytes(),
            );
            hash.update(&[0]);
            hash.update(&std::fs::read(path).unwrap());
            hash.update(&[0]);
        }
    }
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let mut hash = blake3::Hasher::new();
    for dependency in dependencies {
        hash.update(dependency.as_bytes()).update(&[0]);
    }
    for name in ["src", "Cargo.toml", "build.rs"] {
        let path = root.join(name);
        if path.exists() {
            visit(&root, &path, &mut hash);
        }
    }
    println!(
        "cargo::rustc-env=INSLAG_ASSET_IMPLEMENTATION={}",
        hash.finalize()
    );
}
