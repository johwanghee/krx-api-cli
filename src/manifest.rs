use std::collections::BTreeMap;
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct ApiManifest {
    pub generated_at: String,
    pub source: ManifestSource,
    pub category_count: usize,
    pub api_count: usize,
    pub categories: Vec<Category>,
    pub apis: Vec<ApiEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ManifestSource {
    pub service_list_url: String,
    pub spec_directory: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Category {
    pub id: String,
    pub path_segment: String,
    pub label: String,
    pub description: String,
    pub api_count: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ApiEntry {
    pub id: String,
    pub category_id: String,
    pub path_segment: String,
    pub api_id: String,
    pub command_name: String,
    pub display_name: String,
    pub description: String,
    pub api_path: String,
    pub sample_path: String,
    pub http_method: String,
    pub source_file: String,
    pub params: Vec<ApiParam>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ApiParam {
    pub name: String,
    pub cli_name: String,
    pub r#type: String,
    pub required: bool,
    pub description: String,
}

impl ApiManifest {
    pub fn category_by_name(&self, name: &str) -> Option<&Category> {
        self.categories.iter().find(|category| category.id == name)
    }

    pub fn category_entries(&self, category_id: &str) -> Vec<&ApiEntry> {
        self.apis
            .iter()
            .filter(|entry| entry.category_id == category_id)
            .collect()
    }

    pub fn entry_by_command(&self, category_id: &str, command_name: &str) -> Option<&ApiEntry> {
        self.apis
            .iter()
            .find(|entry| entry.category_id == category_id && entry.command_name == command_name)
    }

    pub fn category_counts(&self) -> BTreeMap<&str, usize> {
        self.categories
            .iter()
            .map(|category| (category.id.as_str(), category.api_count))
            .collect()
    }
}

pub fn load_manifest() -> Result<&'static ApiManifest> {
    static MANIFEST: OnceLock<ApiManifest> = OnceLock::new();
    if let Some(manifest) = MANIFEST.get() {
        return Ok(manifest);
    }

    let parsed: ApiManifest = serde_json::from_str(include_str!("../data/krx_api_manifest.json"))
        .context("failed to parse embedded API manifest")?;
    let _ = MANIFEST.set(parsed);

    MANIFEST
        .get()
        .ok_or_else(|| anyhow!("failed to initialize embedded API manifest"))
}
