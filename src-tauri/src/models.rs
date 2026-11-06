use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::service::ServiceKind;

const DEFAULT_EMBEDDING_MODEL_NAME: &str = "embeddinggemma-2-Q8_0.gguf";
const DEFAULT_RERANK_MODEL_NAME: &str = "Qwen3-VL-Reranker-2B.Q8_0.gguf";
pub const EMBEDDINGGEMMA_MIN_LLAMA_BUILD: u64 = 11457;
pub const QWEN_VL_RERANK_MIN_LLAMA_BUILD: u64 = 12110;

pub fn qwen_vl_rerank_projector_path(model_path: &Path) -> Option<PathBuf> {
    model_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| name.eq_ignore_ascii_case(DEFAULT_RERANK_MODEL_NAME))
        .map(|_| model_path.with_file_name("Qwen3-VL-Reranker-2B.mmproj-f16.gguf"))
}

pub fn embeddinggemma_projector_path(model_path: &Path) -> Option<PathBuf> {
    model_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| name.to_lowercase().starts_with("embeddinggemma-2-"))
        .map(|name| model_path.with_file_name(format!("mmproj-{name}")))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCatalog {
    pub rerank: Vec<ModelInfo>,
    pub embedding: Vec<ModelInfo>,
}

pub fn discover_models(
    exe_dir: &Path,
    configured_model: Option<&Path>,
) -> anyhow::Result<Vec<ModelInfo>> {
    let models_dir = exe_dir.join("models");
    if !models_dir.exists() {
        return Ok(Vec::new());
    }

    let configured_model = configured_model.and_then(|path| dunce::canonicalize(path).ok());

    let mut models = Vec::new();
    for entry in fs::read_dir(&models_dir).context("failed to read models directory")? {
        let entry = entry.context("failed to read model directory entry")?;
        let path = entry.path();

        if !entry
            .file_type()
            .context("failed to read model directory entry type")?
            .is_file()
        {
            continue;
        }

        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
        {
            continue;
        }

        let path = dunce::canonicalize(&path).context("failed to canonicalize model path")?;
        let metadata = path.metadata().context("failed to read model metadata")?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .context("model path has no filename")?;
        if name.to_lowercase().starts_with("mmproj-") || name.to_lowercase().contains(".mmproj-") {
            continue;
        }
        let selected = configured_model
            .as_ref()
            .is_some_and(|configured_model| configured_model == &path);

        models.push(ModelInfo {
            name,
            path,
            size_bytes: metadata.len(),
            selected,
        });
    }

    models.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(models)
}

pub fn discover_model_catalog(
    exe_dir: &Path,
    rerank_model: Option<&Path>,
    embedding_model: Option<&Path>,
) -> anyhow::Result<ModelCatalog> {
    Ok(ModelCatalog {
        rerank: discover_models(exe_dir, rerank_model)?,
        embedding: discover_models(exe_dir, embedding_model)?,
    })
}

pub fn choose_default_model(models: &[ModelInfo]) -> Option<ModelInfo> {
    choose_default_model_for_service(ServiceKind::Rerank, models)
}

pub fn choose_default_model_for_service(
    service: ServiceKind,
    models: &[ModelInfo],
) -> Option<ModelInfo> {
    models
        .iter()
        .find(|model| model.selected)
        .or_else(|| {
            let default_name = match service {
                ServiceKind::Embedding => DEFAULT_EMBEDDING_MODEL_NAME,
                ServiceKind::Rerank => DEFAULT_RERANK_MODEL_NAME,
            };
            models
                .iter()
                .find(|model| model.name.eq_ignore_ascii_case(default_name))
        })
        .or_else(|| {
            models.iter().find(|model| {
                let name = model.name.to_lowercase();
                match service {
                    ServiceKind::Rerank => name.contains("rerank") || name.contains("reranker"),
                    ServiceKind::Embedding => name.contains("embedding") || name.contains("embed"),
                }
            })
        })
        .or_else(|| models.first())
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discovers_only_gguf_files_in_models_dir_sorted_with_size() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let models_dir = temp_dir.path().join("models");
        fs::create_dir(&models_dir).expect("models dir should be created");
        fs::write(models_dir.join("zeta.GGUF"), b"12345").expect("model should be written");
        fs::write(models_dir.join("Alpha.gguf"), b"123").expect("model should be written");
        fs::write(models_dir.join("notes.txt"), b"not a model").expect("file should be written");
        fs::write(
            models_dir.join("mmproj-embeddinggemma-2-Q8_0.gguf"),
            b"projector",
        )
        .expect("projector should be written");
        fs::write(
            models_dir.join("Qwen3-VL-Reranker-2B.mmproj-f16.gguf"),
            b"projector",
        )
        .unwrap();
        fs::create_dir(models_dir.join("nested.gguf")).expect("nested dir should be created");
        fs::write(
            models_dir.join("nested.gguf").join("ignored.gguf"),
            b"ignored",
        )
        .expect("nested model should be written");

        let models = discover_models(temp_dir.path(), None).expect("models should be discovered");

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "Alpha.gguf");
        assert_eq!(
            models[0].path,
            dunce::canonicalize(models_dir.join("Alpha.gguf")).unwrap()
        );
        assert_eq!(models[0].size_bytes, 3);
        assert!(!models[0].selected);
        assert_eq!(models[1].name, "zeta.GGUF");
        assert_eq!(
            models[1].path,
            dunce::canonicalize(models_dir.join("zeta.GGUF")).unwrap()
        );
        assert_eq!(models[1].size_bytes, 5);
        assert!(!models[1].selected);
    }

    #[test]
    fn selects_configured_model_when_present() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let models_dir = temp_dir.path().join("models");
        fs::create_dir(&models_dir).expect("models dir should be created");
        let selected_path = models_dir.join("bge-reranker.gguf");
        fs::write(models_dir.join("other.gguf"), b"other").expect("model should be written");
        fs::write(&selected_path, b"selected").expect("model should be written");

        let configured_path =
            dunce::canonicalize(&selected_path).expect("path should canonicalize");
        let models =
            discover_models(temp_dir.path(), Some(&configured_path)).expect("models should load");

        assert_eq!(models.iter().filter(|model| model.selected).count(), 1);
        assert!(models
            .iter()
            .any(|model| model.name == "bge-reranker.gguf" && model.selected));
    }

    #[test]
    fn stale_configured_model_path_does_not_abort_discovery() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let models_dir = temp_dir.path().join("models");
        fs::create_dir(&models_dir).expect("models dir should be created");
        let valid_path = models_dir.join("valid.gguf");
        fs::write(&valid_path, b"valid").expect("model should be written");
        let missing_path = models_dir.join("missing.gguf");

        let models =
            discover_models(temp_dir.path(), Some(&missing_path)).expect("models should load");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name, "valid.gguf");
        assert_eq!(models[0].path, dunce::canonicalize(valid_path).unwrap());
        assert_eq!(models[0].size_bytes, 5);
        assert!(!models[0].selected);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn discovered_model_paths_are_windows_display_safe() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let models_dir = temp_dir.path().join("models");
        fs::create_dir(&models_dir).expect("models dir should be created");
        fs::write(models_dir.join("valid.gguf"), b"valid").expect("model should be written");

        let models = discover_models(temp_dir.path(), None).expect("models should load");

        assert_eq!(models.len(), 1);
        let discovered_path = models[0].path.display().to_string();
        assert!(
            !discovered_path.starts_with(r"\\?\"),
            "model path should be display-safe, got {discovered_path}"
        );
    }

    #[test]
    fn default_selection_prefers_selected_then_reranker_then_first_model() {
        let first = ModelInfo {
            name: "alpha.gguf".to_string(),
            path: "alpha.gguf".into(),
            size_bytes: 1,
            selected: false,
        };
        let reranker = ModelInfo {
            name: "BGE-RERANKER.gguf".to_string(),
            path: "BGE-RERANKER.gguf".into(),
            size_bytes: 2,
            selected: false,
        };
        let selected = ModelInfo {
            name: "selected.gguf".to_string(),
            path: "selected.gguf".into(),
            size_bytes: 3,
            selected: true,
        };

        assert_eq!(
            choose_default_model(&[first.clone(), reranker.clone()]),
            Some(reranker)
        );
        assert_eq!(
            choose_default_model(&[first.clone(), selected.clone()]),
            Some(selected)
        );
        assert_eq!(choose_default_model(&[first.clone()]), Some(first));
        assert_eq!(choose_default_model(&[]), None);
    }

    #[test]
    fn default_embedding_selection_prefers_embedding_names() {
        let first = ModelInfo {
            name: "alpha.gguf".to_string(),
            path: "alpha.gguf".into(),
            size_bytes: 1,
            selected: false,
        };
        let reranker = ModelInfo {
            name: "bge-reranker.gguf".to_string(),
            path: "bge-reranker.gguf".into(),
            size_bytes: 2,
            selected: false,
        };
        let embedding = ModelInfo {
            name: "Qwen3-Embedding-0.6B-Q8_0.gguf".to_string(),
            path: "Qwen3-Embedding-0.6B-Q8_0.gguf".into(),
            size_bytes: 3,
            selected: false,
        };
        let selected = ModelInfo {
            name: "selected.gguf".to_string(),
            path: "selected.gguf".into(),
            size_bytes: 4,
            selected: true,
        };

        assert_eq!(
            choose_default_model_for_service(
                ServiceKind::Embedding,
                &[first.clone(), reranker, embedding.clone()]
            ),
            Some(embedding)
        );
        assert_eq!(
            choose_default_model_for_service(ServiceKind::Embedding, &[first, selected.clone()]),
            Some(selected)
        );
    }

    #[test]
    fn default_embedding_prefers_gemma_over_qwen_but_preserves_selection() {
        let qwen = ModelInfo {
            name: "Qwen3-Embedding-0.6B-Q8_0.gguf".to_string(),
            path: "Qwen3-Embedding-0.6B-Q8_0.gguf".into(),
            size_bytes: 1,
            selected: false,
        };
        let gemma = ModelInfo {
            name: DEFAULT_EMBEDDING_MODEL_NAME.to_string(),
            path: DEFAULT_EMBEDDING_MODEL_NAME.into(),
            size_bytes: 1,
            selected: false,
        };
        assert_eq!(
            choose_default_model_for_service(
                ServiceKind::Embedding,
                &[qwen.clone(), gemma.clone()]
            ),
            Some(gemma.clone())
        );
        let selected_qwen = ModelInfo {
            selected: true,
            ..qwen
        };
        assert_eq!(
            choose_default_model_for_service(
                ServiceKind::Embedding,
                &[gemma, selected_qwen.clone()]
            ),
            Some(selected_qwen)
        );
    }

    #[test]
    fn discovers_model_catalog_with_independent_selected_flags() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let models_dir = temp_dir.path().join("models");
        fs::create_dir(&models_dir).expect("models dir should be created");
        let rerank_path = models_dir.join("bge-reranker.gguf");
        let embedding_path = models_dir.join("Qwen3-Embedding.gguf");
        fs::write(&rerank_path, b"rerank").expect("rerank model should be written");
        fs::write(&embedding_path, b"embedding").expect("embedding model should be written");

        let catalog =
            discover_model_catalog(temp_dir.path(), Some(&rerank_path), Some(&embedding_path))
                .expect("model catalog should load");

        assert!(catalog
            .rerank
            .iter()
            .any(|model| model.name == "bge-reranker.gguf" && model.selected));
        assert!(catalog
            .embedding
            .iter()
            .any(|model| model.name == "Qwen3-Embedding.gguf" && model.selected));
    }

    #[test]
    fn missing_models_dir_returns_empty_vec() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");

        let models = discover_models(temp_dir.path(), None).expect("missing dir should be ok");

        assert!(models.is_empty());
    }

    #[test]
    fn default_rerank_prefers_multimodal_but_preserves_saved_selection() {
        let mut bge = ModelInfo {
            name: "bge-reranker.gguf".into(),
            path: "bge-reranker.gguf".into(),
            size_bytes: 1,
            selected: false,
        };
        let vl = ModelInfo {
            name: DEFAULT_RERANK_MODEL_NAME.into(),
            path: DEFAULT_RERANK_MODEL_NAME.into(),
            size_bytes: 1,
            selected: false,
        };
        assert_eq!(
            choose_default_model(&[bge.clone(), vl.clone()]),
            Some(vl.clone())
        );
        bge.selected = true;
        assert_eq!(choose_default_model(&[bge.clone(), vl]), Some(bge));
    }
}
