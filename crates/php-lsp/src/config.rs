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

/// External tool configuration (`php-lsp.tools.*`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolsConfig {
    pub phpstan: php_tools::ToolConfig,
    pub psalm: php_tools::ToolConfig,
    pub phpcs: php_tools::ToolConfig,
    pub phpcbf: php_tools::ToolConfig,
    pub php_cs_fixer: php_tools::ToolConfig,
    pub phpunit: php_tools::ToolConfig,
}

/// Which formatter handles formatting requests (`php-lsp.format.provider`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormatProvider {
    #[default]
    Builtin,
    PhpCsFixer,
    Phpcbf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    pub tools: ToolsConfig,
    pub format_provider: FormatProvider,
    pub php_version: PhpVersion,
    pub diagnostics_enabled: bool,
    pub diagnostics: php_ide::DiagnosticsConfig,
    pub inlay_hints: php_ide::InlayHintsConfig,
    /// Files larger than this (in bytes) are not analysed.
    pub max_file_size: u64,
    pub associations: Vec<String>,
    pub exclude: Vec<String>,
    pub stubs: Vec<String>,
    pub insert_use_declaration: bool,
    pub format_enable: bool,
    /// `diagnostics.run`: publish on save instead of on every change.
    pub diagnostics_on_save: bool,
    /// `environment.includePaths`: extra directories indexed as dependencies.
    pub include_paths: Vec<String>,
    pub format_braces: php_ide::BraceStyle,
    pub max_completion_items: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        let scan = php_ide::ScanConfig::default();
        let ide = php_ide::Config::default();
        ServerConfig {
            tools: ToolsConfig::default(),
            format_provider: FormatProvider::Builtin,
            php_version: PhpVersion::default(),
            diagnostics_enabled: true,
            diagnostics: php_ide::DiagnosticsConfig::default(),
            inlay_hints: php_ide::InlayHintsConfig::default(),
            max_file_size: scan.max_size,
            associations: scan.associations,
            exclude: scan.exclude,
            stubs: ide.stubs,
            insert_use_declaration: ide.auto_import,
            format_enable: true,
            diagnostics_on_save: false,
            include_paths: Vec::new(),
            format_braces: php_ide::BraceStyle::Psr12,
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
            diagnostics: php_ide::DiagnosticsConfig {
                enable: self.diagnostics_enabled,
                ..self.diagnostics.clone()
            },
            stubs: self.stubs.clone(),
            auto_import: self.insert_use_declaration,
            max_completion_items: self.max_completion_items,
            inlay_hints: self.inlay_hints.clone(),
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
            let h = &mut self.inlay_hints;
            for (key, flag) in [
                ("inlayHint.parameterNames", &mut h.parameter_names),
                ("inlayHint.returnTypes", &mut h.return_types),
                ("inlayHint.parameterTypes", &mut h.parameter_types),
            ] {
                if let Some(v) = get(&root, key).and_then(Value::as_bool) {
                    *flag = v;
                }
            }
            let d = &mut self.diagnostics;
            for (key, flag) in [
                ("diagnostics.syntax", &mut d.syntax),
                ("diagnostics.undefinedTypes", &mut d.undefined_types),
                ("diagnostics.undefinedFunctions", &mut d.undefined_functions),
                ("diagnostics.undefinedConstants", &mut d.undefined_constants),
                ("diagnostics.undefinedClassConstants", &mut d.undefined_class_constants),
                ("diagnostics.undefinedMethods", &mut d.undefined_methods),
                ("diagnostics.undefinedProperties", &mut d.undefined_properties),
                ("diagnostics.undefinedVariables", &mut d.undefined_variables),
                ("diagnostics.unusedSymbols", &mut d.unused_symbols),
                ("diagnostics.deprecated", &mut d.deprecated),
                ("diagnostics.argumentCount", &mut d.argument_count),
                ("diagnostics.implementationErrors", &mut d.implementation_errors),
            ] {
                if let Some(v) = get(&root, key).and_then(Value::as_bool) {
                    *flag = v;
                }
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
            for (name, tool) in [
                ("phpstan", &mut self.tools.phpstan),
                ("psalm", &mut self.tools.psalm),
                ("phpcs", &mut self.tools.phpcs),
                ("phpcbf", &mut self.tools.phpcbf),
                ("phpCsFixer", &mut self.tools.php_cs_fixer),
                ("phpunit", &mut self.tools.phpunit),
            ] {
                if let Some(v) = get(&root, &format!("tools.{name}.enable")).and_then(Value::as_bool) {
                    tool.enabled = v;
                }
                if let Some(v) = get(&root, &format!("tools.{name}.path")).and_then(Value::as_str) {
                    tool.path = if v.is_empty() { None } else { Some(v.to_string()) };
                }
                if let Some(v) = get(&root, &format!("tools.{name}.args")).and_then(string_list) {
                    tool.args = v;
                }
            }
            if let Some(v) = get(&root, "format.provider").and_then(Value::as_str) {
                self.format_provider = match v.to_ascii_lowercase().as_str() {
                    "php-cs-fixer" | "phpcsfixer" => FormatProvider::PhpCsFixer,
                    "phpcbf" => FormatProvider::Phpcbf,
                    _ => FormatProvider::Builtin,
                };
            }
            if let Some(v) = get(&root, "diagnostics.run").and_then(Value::as_str) {
                self.diagnostics_on_save = v.eq_ignore_ascii_case("onSave");
            }
            if let Some(v) = get(&root, "environment.includePaths").and_then(string_list) {
                self.include_paths = v;
            }
            if let Some(v) = get(&root, "format.enable").and_then(Value::as_bool) {
                self.format_enable = v;
            }
            if let Some(v) = get(&root, "format.braces").and_then(Value::as_str) {
                self.format_braces = match v.to_ascii_lowercase().as_str() {
                    "allman" => php_ide::BraceStyle::Allman,
                    "k&r" | "k_and_r" | "kandr" => php_ide::BraceStyle::KAndR,
                    _ => php_ide::BraceStyle::Psr12,
                };
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

    #[test]
    fn reads_settings_merged_by_vscode_middleware() {
        let mut c = ServerConfig::default();
        c.update(&json!({"intelephense": {
            "files": {"associations": ["*.php", "*.phtml", "*.inc"], "exclude": ["**/.git/**", "**/node_modules/**"], "maxSize": 1000000},
            "diagnostics": {"run": "onSave", "undefinedTypes": true},
            "shortOpenEchoAutoClose": true, "runtime": null, "maxMemory": 2048,
            "telemetry": {"enabled": false}, "licenceKey": null
        }}));
        assert!(c.diagnostics_on_save);
        assert_eq!(c.associations, vec!["*.php", "*.phtml", "*.inc"]);
        assert_eq!(c.exclude.len(), 2);
        c.update(&json!({"intelephense": {"diagnostics": {"run": null}}}));
        assert!(c.diagnostics_on_save);
    }
}
