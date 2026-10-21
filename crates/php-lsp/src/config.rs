//! Server settings.
//!
//! Settings are read from `initializationOptions` and
//! `workspace/didChangeConfiguration`. Both the `intelephense.*` namespace
//! (so existing editor configurations keep working) and `php-lsp.*` are
//! understood; `php-lsp.*` wins if both are set.

use php_ide::PhpVersion;
use serde_json::Value;

/// Sections the server reads settings from, in increasing priority.
pub const SECTIONS: &[&str] = &["intelephense", "php-lsp"];

#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    pub php_version: PhpVersion,
    pub diagnostics_enabled: bool,
    /// Files larger than this (in bytes) are not analysed.
    pub max_file_size: u64,
    pub associations: Vec<String>,
    pub exclude: Vec<String>,
    pub stubs: Vec<String>,
    pub insert_use_declaration: bool,
    pub max_completion_items: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        let scan = php_ide::ScanConfig::default();
        let ide = php_ide::Config::default();
        ServerConfig {
            php_version: PhpVersion::default(),
            diagnostics_enabled: true,
            max_file_size: scan.max_size,
            associations: scan.associations,
            exclude: scan.exclude,
            stubs: ide.stubs,
            insert_use_declaration: ide.auto_import,
            max_completion_items: ide.max_completion_items,
        }
    }
}

fn string_list(v: &Value) -> Option<Vec<String>> {
    Some(v.as_array()?.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
}

impl ServerConfig {
    pub fn ide_config(&self) -> php_ide::Config {
        php_ide::Config {
            php_version: self.php_version,
            syntax_diagnostics: self.diagnostics_enabled,
            stubs: self.stubs.clone(),
            auto_import: self.insert_use_declaration,
            max_completion_items: self.max_completion_items,
        }
    }

    pub fn scan_config(&self) -> php_ide::ScanConfig {
        php_ide::ScanConfig {
            associations: self.associations.clone(),
            exclude: self.exclude.clone(),
            max_size: self.max_file_size,
        }
    }

    /// Applies a settings object. Unknown keys are ignored.
    pub fn update(&mut self, value: &Value) {
        for section in SECTIONS {
            let root = section_value(value, section);
            let Some(root) = root else { continue };
            if let Some(v) = get(&root, "environment.phpVersion").and_then(Value::as_str) {
                if let Ok(v) = v.parse() {
                    self.php_version = v;
                }
            }
            if let Some(v) = get(&root, "diagnostics.enable").and_then(Value::as_bool) {
                self.diagnostics_enabled = v;
            }
            if let Some(v) = get(&root, "files.maxSize").and_then(Value::as_u64) {
                self.max_file_size = v;
            }
            if let Some(v) = get(&root, "files.associations").and_then(string_list) {
                self.associations = v;
            }
            if let Some(v) = get(&root, "files.exclude").and_then(string_list) {
                self.exclude = v;
            }
            if let Some(v) = get(&root, "stubs").and_then(string_list) {
                self.stubs = v;
            }
            if let Some(v) = get(&root, "completion.insertUseDeclaration").and_then(Value::as_bool) {
                self.insert_use_declaration = v;
            }
            if let Some(v) = get(&root, "completion.maxItems").and_then(Value::as_u64) {
                self.max_completion_items = v as usize;
            }
        }
    }
}

/// Extracts a settings section, accepting both nested objects
/// (`{"intelephense": {"files": {...}}}`) and flat dotted keys
/// (`{"intelephense.files.maxSize": 1}`).
fn section_value(value: &Value, section: &str) -> Option<Value> {
    let obj = value.as_object()?;
    let mut out = serde_json::Map::new();
    if let Some(Value::Object(nested)) = obj.get(section) {
        out.extend(nested.clone());
    }
    let prefix = format!("{section}.");
    for (k, v) in obj {
        if let Some(rest) = k.strip_prefix(&prefix) {
            out.insert(rest.to_string(), v.clone());
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(Value::Object(out))
    }
}

/// Looks up `a.b.c` either as nested objects or as a literal dotted key.
fn get<'v>(value: &'v Value, path: &str) -> Option<&'v Value> {
    if let Some(v) = value.get(path) {
        return Some(v);
    }
    let (head, rest) = path.split_once('.')?;
    get(value.get(head)?, rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_intelephense_settings() {
        let mut c = ServerConfig::default();
        c.update(
            &json!({"intelephense": {"environment": {"phpVersion": "7.4.3"}, "diagnostics.enable": false}}),
        );
        assert_eq!(c.php_version, PhpVersion::PHP_7_4);
        assert!(!c.diagnostics_enabled);
        c.update(&json!({"php-lsp.environment.phpVersion": "8.2"}));
        assert_eq!(c.php_version, PhpVersion::PHP_8_2);
    }
}
