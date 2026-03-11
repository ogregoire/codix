use std::path::Path;
use crate::model::*;

pub mod java;

pub trait LanguagePlugin {
    fn name(&self) -> &str;
    fn display_name(&self) -> &str { self.name() }
    fn can_handle(&self, path: &Path) -> bool;
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

    pub fn all_plugins(&self) -> Vec<&dyn LanguagePlugin> {
        self.plugins.iter().map(|p| p.as_ref()).collect()
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

    pub fn plugins_for_languages(&self, languages: &[String]) -> Vec<&dyn LanguagePlugin> {
        self.plugins.iter()
            .filter(|p| languages.iter().any(|l| l == p.name()))
            .map(|p| p.as_ref())
            .collect()
    }
}
