use crate::{Result, digest, invalid};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Deserialize)]
pub(crate) struct Lockfile {
    version: u32,
    #[serde(default, rename = "package")]
    packages: Vec<Package>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Package {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
    #[serde(default)]
    dependencies: Vec<String>,
}

type DependencyIdentity = (String, String, Option<String>);

impl Lockfile {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self> {
        let lock: Self = toml::from_str(std::str::from_utf8(bytes)?)?;
        if !matches!(lock.version, 3 | 4) {
            return Err(invalid("unsupported Cargo.lock version"));
        }
        Ok(lock)
    }

    pub(crate) fn fingerprint(&self, operation: &str) -> Result<String> {
        let selected = self.select(operation)?;
        let mut packages = Vec::new();
        for index in selected {
            let package = &self.packages[index];
            let mut dependencies = package
                .dependencies
                .iter()
                .map(|reference| {
                    let dependency = &self.packages[self.resolve(reference)?];
                    Ok((&dependency.name, &dependency.version, &dependency.source))
                })
                .collect::<Result<Vec<_>>>()?;
            dependencies.sort();
            dependencies.dedup();
            packages.push((
                &package.name,
                &package.version,
                &package.source,
                &package.checksum,
                dependencies,
            ));
        }
        packages.sort();
        Ok(digest(&serde_json::to_vec(&packages)?))
    }

    pub(crate) fn validate_migration(
        &self,
        previous: &Self,
        providers: &BTreeSet<String>,
    ) -> Result<()> {
        let mut selected = BTreeSet::new();
        let mut previous_selected = BTreeSet::new();
        for provider in providers {
            selected.extend(self.select(provider)?);
            previous_selected.extend(previous.select(provider)?);
        }
        for index in selected {
            let current = &self.packages[index];
            let identity = migration_identity(current, providers);
            let dependencies = self.migration_edges(current, providers)?;
            let mut compatible = false;
            for index in &previous_selected {
                let prior = &previous.packages[*index];
                if current.checksum == prior.checksum
                    && identity == migration_identity(prior, providers)
                    && dependencies == previous.migration_edges(prior, providers)?
                {
                    compatible = true;
                    break;
                }
            }
            if !compatible {
                return Err(invalid(format!(
                    "asset dependency {} {} changed; regenerate assets",
                    current.name, current.version
                )));
            }
        }
        Ok(())
    }

    fn migration_edges(
        &self,
        package: &Package,
        providers: &BTreeSet<String>,
    ) -> Result<BTreeSet<DependencyIdentity>> {
        let mut dependencies = BTreeSet::new();
        for reference in &package.dependencies {
            let dependency = &self.packages[self.resolve(reference)?];
            // The v2 lock parser is metadata-only; it cannot change processor output.
            if package.name == "inslag_asset_pipeline" && dependency.name == "toml" {
                continue;
            }
            dependencies.insert(migration_identity(dependency, providers));
        }
        Ok(dependencies)
    }

    fn select(&self, operation: &str) -> Result<BTreeSet<usize>> {
        let provider = normalize(operation.split("::").next().unwrap_or(operation));
        let mut pending = self
            .packages
            .iter()
            .enumerate()
            .filter(|(_, package)| normalize(&package.name) == provider)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        // Custom lib/bin target names need not match their package. Unknown providers stay conservative.
        if pending.is_empty() {
            pending.extend(0..self.packages.len());
        }
        let mut selected = BTreeSet::new();
        while let Some(index) = pending.pop() {
            if selected.insert(index) {
                for reference in &self.packages[index].dependencies {
                    pending.push(self.resolve(reference)?);
                }
            }
        }
        Ok(selected)
    }

    fn resolve(&self, reference: &str) -> Result<usize> {
        let (identity, source) = reference
            .split_once(" (")
            .map_or((reference, None), |(identity, source)| {
                (identity, source.strip_suffix(')'))
            });
        let mut fields = identity.split_whitespace();
        let name = fields
            .next()
            .ok_or_else(|| invalid("empty locked dependency"))?;
        let version = fields.next();
        if fields.next().is_some() {
            return Err(invalid(format!("invalid locked dependency {reference}")));
        }
        let matches = self
            .packages
            .iter()
            .enumerate()
            .filter(|(_, package)| {
                package.name == name
                    && version.is_none_or(|version| package.version == version)
                    && source.is_none_or(|source| package.source.as_deref() == Some(source))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [index] => Ok(*index),
            _ => Err(invalid(format!(
                "missing or ambiguous locked dependency {reference}"
            ))),
        }
    }
}

fn migration_identity(package: &Package, providers: &BTreeSet<String>) -> DependencyIdentity {
    // Provider implementations are verified against every old job receipt separately.
    let provider =
        providers.contains(&normalize(&package.name)) || package.name == "inslag_asset_pipeline";
    let source = package.source.as_ref().map(|source| {
        if provider && source.starts_with("git+") {
            git_repository(source).into()
        } else {
            source.clone()
        }
    });
    (package.name.clone(), package.version.clone(), source)
}

fn normalize(name: &str) -> String {
    name.replace('-', "_")
}

fn git_repository(source: &str) -> &str {
    source.split(['?', '#']).next().unwrap_or(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK: &str = r#"
version = 4
[[package]]
name = "asset-renderer"
version = "1.0.0"
dependencies = ["codec"]
[[package]]
name = "codec"
version = "2.0.0"
source = "registry+https://example.invalid/index"
checksum = "codec-checksum"
[[package]]
name = "game"
version = "0.1.0"
dependencies = []
"#;

    #[test]
    fn unrelated_game_dependencies_and_lockfile_formatting_do_not_change_renderers() {
        let before = Lockfile::parse(LOCK.as_bytes())
            .unwrap()
            .fingerprint("asset_renderer::Render")
            .unwrap();
        let after = format!(
            "{}\n[[package]]\nname='test-helper'\nversion='1.0.0'\n",
            LOCK.replace("dependencies = []", "dependencies = ['test-helper']")
        );
        assert_eq!(
            before,
            Lockfile::parse(after.as_bytes())
                .unwrap()
                .fingerprint("asset_renderer::Render")
                .unwrap()
        );
        assert_eq!(
            before,
            Lockfile::parse(LOCK.replace("[\"codec\"]", "['codec 2.0.0']").as_bytes())
                .unwrap()
                .fingerprint("asset_renderer::Render")
                .unwrap()
        );
    }

    #[test]
    fn transitive_versions_checksums_and_sources_invalidate_the_renderer() {
        let before = Lockfile::parse(LOCK.as_bytes())
            .unwrap()
            .fingerprint("asset_renderer::Render")
            .unwrap();
        for after in [
            LOCK.replace("2.0.0", "3.0.0"),
            LOCK.replace("codec-checksum", "different-checksum"),
            LOCK.replace("example.invalid", "other.invalid"),
        ] {
            assert_ne!(
                before,
                Lockfile::parse(after.as_bytes())
                    .unwrap()
                    .fingerprint("asset_renderer::Render")
                    .unwrap()
            );
        }
    }

    #[test]
    fn ambiguous_edges_fail_and_qualified_edges_resolve() {
        let lock = format!("{LOCK}\n[[package]]\nname='codec'\nversion='3.0.0'\n");
        assert!(
            Lockfile::parse(lock.as_bytes())
                .unwrap()
                .fingerprint("asset_renderer::Render")
                .is_err()
        );
        assert!(
            Lockfile::parse(lock.replace("[\"codec\"]", "['codec 2.0.0']").as_bytes())
                .unwrap()
                .fingerprint("asset_renderer::Render")
                .is_ok()
        );
    }

    #[test]
    fn missing_edges_fail_instead_of_omitting_inputs() {
        assert!(
            Lockfile::parse(LOCK.replace("[\"codec\"]", "['missing']").as_bytes())
                .unwrap()
                .fingerprint("asset_renderer::Render")
                .is_err()
        );
    }

    #[test]
    fn migration_does_not_adopt_a_codec_previously_used_only_by_an_unrelated_package() {
        let previous = format!(
            "{}\n[[package]]\nname='codec'\nversion='3.0.0'\n",
            LOCK.replace("[\"codec\"]", "['codec 2.0.0']")
        );
        let current = previous.replace("['codec 2.0.0']", "['codec 3.0.0']");
        let previous = Lockfile::parse(previous.as_bytes()).unwrap();
        let current = Lockfile::parse(current.as_bytes()).unwrap();
        assert!(
            current
                .validate_migration(&previous, &["asset_renderer".into()].into())
                .is_err()
        );
    }

    #[test]
    fn migration_cannot_switch_to_another_providers_codec_version() {
        let previous = format!(
            "{}\n[[package]]\nname='codec'\nversion='3.0.0'\n[[package]]\nname='other-renderer'\nversion='1.0.0'\ndependencies=['codec 3.0.0']\n",
            LOCK.replace("[\"codec\"]", "['codec 2.0.0']")
        );
        let current = previous.replacen("['codec 2.0.0']", "['codec 3.0.0']", 1);
        let previous = Lockfile::parse(previous.as_bytes()).unwrap();
        let current = Lockfile::parse(current.as_bytes()).unwrap();
        assert!(
            current
                .validate_migration(
                    &previous,
                    &["asset_renderer".into(), "other_renderer".into()].into()
                )
                .is_err()
        );
    }
}
