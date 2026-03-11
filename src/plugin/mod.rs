use std::path::Path;
use crate::model::*;

pub mod java;

pub trait LanguagePlugin {
    fn name(&self) -> &str;
    fn display_name(&self) -> &str { self.name() }
    fn file_extensions(&self) -> &[&str];
    fn tree_sitter_language(&self) -> tree_sitter::Language;
    fn extract_symbols(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &Path,
    ) -> ExtractionResult;
}

pub struct PluginRegistry {
    plugins: Vec<Box<dyn LanguagePlugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        let mut registry = Self { plugins: Vec::new() };
        registry.register(Box::new(java::JavaPlugin));
        registry
    }

    pub fn register(&mut self, plugin: Box<dyn LanguagePlugin>) {
        self.plugins.push(plugin);
    }

    pub fn plugin_for_extension(&self, ext: &str) -> Option<&dyn LanguagePlugin> {
        self.plugins.iter()
            .find(|p| p.file_extensions().contains(&ext))
            .map(|p| p.as_ref())
    }

    pub fn all_extensions(&self) -> Vec<&str> {
        self.plugins.iter().flat_map(|p| p.file_extensions()).copied().collect()
    }

    pub fn display_name_for(&self, name: &str) -> String {
        self.plugins.iter()
            .find(|p| p.name() == name)
            .map(|p| p.display_name().to_string())
            .unwrap_or_else(|| name.to_string())
    }

    pub fn all_language_names(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.name()).collect()
    }

    pub fn extensions_for_languages(&self, languages: &[String]) -> Vec<&str> {
        self.plugins.iter()
            .filter(|p| languages.iter().any(|l| l == p.name()))
            .flat_map(|p| p.file_extensions())
            .copied()
            .collect()
    }
}
