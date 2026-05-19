use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Live2dModel {
    pub manifest_path: PathBuf,
    pub root_dir: PathBuf,
    pub version: u32,
    pub moc: PathBuf,
    pub textures: Vec<PathBuf>,
    pub physics: Option<PathBuf>,
    pub display_info: Option<PathBuf>,
    pub motions: HashMap<String, Vec<ModelMotion>>,
    pub expressions: Vec<ModelExpression>,
    pub groups: Vec<ModelGroup>,
}

#[derive(Debug, Clone)]
pub struct ModelGroup {
    pub target: String,
    pub name: String,
    pub ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ModelMotion {
    pub file: PathBuf,
    pub fade_in_time: Option<f32>,
    pub fade_out_time: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct ModelExpression {
    pub name: String,
    pub file: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ModelManifest {
    version: u32,
    file_references: FileReferences,
    #[serde(default)]
    groups: Vec<ModelGroupManifest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct FileReferences {
    moc: String,
    #[serde(default)]
    textures: Vec<String>,
    physics: Option<String>,
    display_info: Option<String>,
    #[serde(default)]
    motions: HashMap<String, Vec<ModelMotionManifest>>,
    #[serde(default)]
    expressions: Vec<ModelExpressionManifest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ModelMotionManifest {
    file: String,
    fade_in_time: Option<f32>,
    fade_out_time: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ModelExpressionManifest {
    name: String,
    file: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ModelGroupManifest {
    target: String,
    name: String,
    #[serde(default)]
    ids: Vec<String>,
}

impl Live2dModel {
    pub fn load(manifest_path: impl AsRef<Path>) -> Result<Self, String> {
        let manifest_path = manifest_path.as_ref().to_path_buf();
        let root_dir = manifest_path
            .parent()
            .ok_or_else(|| {
                format!(
                    "Model manifest has no parent directory: {}",
                    manifest_path.display()
                )
            })?
            .to_path_buf();

        let manifest_text = fs::read_to_string(&manifest_path)
            .map_err(|error| format!("Failed to read {}: {error}", manifest_path.display()))?;
        let manifest: ModelManifest = serde_json::from_str(&manifest_text)
            .map_err(|error| format!("Failed to parse {}: {error}", manifest_path.display()))?;

        let model = Self {
            moc: root_dir.join(&manifest.file_references.moc),
            textures: manifest
                .file_references
                .textures
                .iter()
                .map(|texture| root_dir.join(texture))
                .collect(),
            physics: manifest
                .file_references
                .physics
                .as_ref()
                .map(|physics| root_dir.join(physics)),
            display_info: manifest
                .file_references
                .display_info
                .as_ref()
                .map(|display_info| root_dir.join(display_info)),
            motions: manifest
                .file_references
                .motions
                .into_iter()
                .map(|(group, motions)| {
                    (
                        group,
                        motions
                            .into_iter()
                            .map(|motion| ModelMotion {
                                file: root_dir.join(motion.file),
                                fade_in_time: motion.fade_in_time,
                                fade_out_time: motion.fade_out_time,
                            })
                            .collect(),
                    )
                })
                .collect(),
            expressions: manifest
                .file_references
                .expressions
                .into_iter()
                .map(|expression| ModelExpression {
                    name: expression.name,
                    file: root_dir.join(expression.file),
                })
                .collect(),
            groups: manifest
                .groups
                .into_iter()
                .map(|group| ModelGroup {
                    target: group.target,
                    name: group.name,
                    ids: group.ids,
                })
                .collect(),
            version: manifest.version,
            manifest_path,
            root_dir,
        };

        model.validate()?;
        Ok(model)
    }

    #[cfg(not(feature = "metal-renderer"))]
    pub fn primary_texture(&self) -> Option<&Path> {
        self.textures.first().map(PathBuf::as_path)
    }

    pub fn summary(&self) -> String {
        let groups = if self.groups.is_empty() {
            "none".to_string()
        } else {
            self.groups
                .iter()
                .map(|group| format!("{}:{}({})", group.target, group.name, group.ids.len()))
                .collect::<Vec<_>>()
                .join(", ")
        };

        format!(
            "Live2D v{} | textures: {} | groups: {}",
            self.version,
            self.textures.len(),
            groups
        )
    }

    fn validate(&self) -> Result<(), String> {
        validate_file("model manifest", &self.manifest_path)?;
        validate_file("moc", &self.moc)?;

        if self.textures.is_empty() {
            return Err(format!(
                "Model {} does not reference any textures",
                self.manifest_path.display()
            ));
        }

        for texture in &self.textures {
            validate_file("texture", texture)?;
        }

        if let Some(physics) = &self.physics {
            validate_file("physics", physics)?;
        }

        if let Some(display_info) = &self.display_info {
            validate_file("display info", display_info)?;
        }

        for (group, motions) in &self.motions {
            for motion in motions {
                validate_file(&format!("motion {group}"), &motion.file)?;
            }
        }

        for expression in &self.expressions {
            validate_file(&format!("expression {}", expression.name), &expression.file)?;
        }

        Ok(())
    }
}

fn validate_file(label: &str, path: &Path) -> Result<(), String> {
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("Missing {label} file: {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::Live2dModel;

    #[test]
    fn loads_public_model_manifest() {
        let model = Live2dModel::load("public/model/0.model3.json")
            .expect("public/model/0.model3.json should load");

        assert_eq!(model.version, 3);
        assert!(model.moc.ends_with("0.moc3"));
        assert_eq!(model.textures.len(), 2);
        assert!(model.physics.is_some());
        assert!(model.display_info.is_some());
        assert!(model.motions.is_empty());
        assert!(model.expressions.is_empty());
    }
}
