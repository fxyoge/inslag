use std::{collections::BTreeMap, path::Path};

use crate::data::{self, Data};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Story {
    pub name: String,
    pub view: String,
    pub data: Data,
}

pub(crate) fn discover(root: &Path) -> Result<BTreeMap<String, Story>, String> {
    fn visit(root: &Path, dir: &Path, result: &mut BTreeMap<String, Story>) -> Result<(), String> {
        for entry in
            std::fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))?
        {
            let entry = entry.map_err(|error| error.to_string())?;
            let ty = entry.file_type().map_err(|error| error.to_string())?;
            let path = entry.path();
            if ty.is_dir() {
                visit(root, &path, result)?;
            } else if ty.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(".story.json"))
            {
                let source = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
                let story =
                    parse(&source).map_err(|error| format!("{}: {error}", path.display()))?;
                result.insert(
                    path.strip_prefix(root)
                        .map_err(|error| error.to_string())?
                        .to_string_lossy()
                        .replace('\\', "/"),
                    story,
                );
            }
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result)?;
    Ok(result)
}

fn parse(source: &str) -> Result<Story, String> {
    let value: serde_json::Value =
        serde_json::from_str(source).map_err(|error| error.to_string())?;
    let object = value.as_object().ok_or("story must be a JSON object")?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "name" | "view" | "data"))
    {
        return Err("unknown story field".into());
    }
    let name = object
        .get("name")
        .and_then(|value| value.as_str())
        .filter(|name| !name.trim().is_empty())
        .ok_or("story requires a nonempty name")?;
    let view = object
        .get("view")
        .and_then(|value| value.as_str())
        .ok_or("story requires a view path")?;
    inslag_kui::source::validate_asset_path(view).map_err(str::to_owned)?;
    if !view.ends_with(".kui") {
        return Err("story view must end in .kui".into());
    }
    let data = object.get("data").ok_or("story requires data")?;
    Ok(Story {
        name: name.into(),
        view: view.into(),
        data: data::decode(&data.to_string())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn story_contract_preserves_structured_data_and_rejects_invalid_metadata() {
        let story = parse(
            r#"{"name":"Empty","view":"ui/roster.kui","data":{"heroes":[],"count":1,"ratio":1.0}}"#,
        )
        .unwrap();
        assert_eq!(story.name, "Empty");
        assert_eq!(story.view, "ui/roster.kui");
        assert_eq!(
            story.data["heroes"],
            inslag_state_tree::LuauValue::Sequence(vec![])
        );
        assert_eq!(
            story.data["count"],
            inslag_state_tree::LuauValue::Integer(1)
        );
        assert_eq!(
            story.data["ratio"],
            inslag_state_tree::LuauValue::Number(1.0)
        );
        for source in [
            r#"{"name":"","view":"a.kui","data":{}}"#,
            r#"{"name":"A","view":"../a.kui","data":{}}"#,
            r#"{"name":"A","view":"a.kui","data":[]}"#,
            r#"{"name":"A","view":"a.kui","data":{},"typo":true}"#,
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn discovery_tracks_edits_deletions_and_separate_story_identity() {
        let root = tempfile::tempdir().unwrap();
        for file in ["a.story.json", "b.story.json"] {
            std::fs::write(
                root.path().join(file),
                r#"{"name":"Same label","view":"ui/a.kui","data":{"x":1}}"#,
            )
            .unwrap();
        }
        std::fs::write(root.path().join("unrelated.json"), "invalid").unwrap();
        let initial = discover(root.path()).unwrap();
        assert_eq!(initial.len(), 2);
        std::fs::write(
            root.path().join("a.story.json"),
            r#"{"name":"Changed","view":"ui/a.kui","data":{"x":2}}"#,
        )
        .unwrap();
        std::fs::remove_file(root.path().join("b.story.json")).unwrap();
        let changed = discover(root.path()).unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(
            changed["a.story.json"].data["x"],
            inslag_state_tree::LuauValue::Integer(2)
        );
        assert_ne!(initial, changed);
    }
}
