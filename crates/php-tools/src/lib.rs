//! Runs external PHP tools and converts their results.
//!
//! * diagnostics: PHPStan, Psalm, PHP_CodeSniffer (`phpcs`)
//! * formatting: php-cs-fixer, phpcbf
//! * tests: PHPUnit
//!
//! Executables are looked up in `<root>/vendor/bin/<tool>`, then on `$PATH`;
//! a configured path (also `*.phar`) always wins.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    PhpStan,
    Psalm,
    Phpcs,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::PhpStan => "phpstan",
            Tool::Psalm => "psalm",
            Tool::Phpcs => "phpcs",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolSeverity {
    Error,
    Warning,
    Information,
}

/// A diagnostic reported by a tool (0-based line, optional columns).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDiagnostic {
    pub line: u32,
    pub column: Option<u32>,
    pub end_line: Option<u32>,
    pub end_column: Option<u32>,
    pub message: String,
    pub severity: ToolSeverity,
    pub code: Option<String>,
    pub source: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolConfig {
    pub enabled: bool,
    /// Path to the executable or `.phar`.
    pub path: Option<String>,
    /// Extra arguments (e.g. `--level=8`, `--standard=PSR12`).
    pub args: Vec<String>,
}

/// Locates a tool: configured path, `vendor/bin`, `$PATH`.
pub fn find_executable(root: &Path, name: &str, configured: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = configured.filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        let p = if p.is_relative() { root.join(p) } else { p };
        return p.exists().then_some(p);
    }
    let vendor = root.join("vendor").join("bin").join(name);
    if vendor.exists() {
        return Some(vendor);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

fn command_for(exe: &Path) -> Command {
    // `.phar` files and vendor/bin proxies run through php
    let is_phar = exe.extension().is_some_and(|e| e == "phar");
    if is_phar {
        let mut c = Command::new(std::env::var("PHP_BINARY").unwrap_or_else(|_| "php".into()));
        c.arg(exe);
        c
    } else {
        Command::new(exe)
    }
}

/// Runs a command with an optional stdin and a timeout, returning stdout.
fn run(mut cmd: Command, stdin: Option<&str>, timeout: Duration) -> Result<String, String> {
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("cannot start {:?}: {e}", cmd.get_program()))?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let input = input.to_string();
        std::thread::spawn(move || {
            let _ = pipe.write_all(input.as_bytes());
        });
    }
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                return Err(format!("{:?} timed out", cmd.get_program()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Runs a diagnostics tool for one file. `text` is the current buffer
/// content (used by tools that read stdin; others analyse the file on disk).
pub fn diagnostics(
    tool: Tool,
    root: &Path,
    file: &Path,
    text: &str,
    config: &ToolConfig,
) -> Result<Vec<ToolDiagnostic>, String> {
    let exe = find_executable(root, tool.name(), config.path.as_deref())
        .ok_or_else(|| format!("{} not found", tool.name()))?;
    let mut cmd = command_for(&exe);
    cmd.current_dir(root);
    let timeout = Duration::from_secs(60);
    let out = match tool {
        Tool::PhpStan => {
            cmd.args(["analyse", "--error-format=json", "--no-progress", "--no-interaction"])
                .args(&config.args)
                .arg(file);
            run(cmd, None, timeout)?
        }
        Tool::Psalm => {
            cmd.args(["--output-format=json", "--no-progress", "--no-cache"]).args(&config.args).arg(file);
            run(cmd, None, timeout)?
        }
        Tool::Phpcs => {
            cmd.args(["--report=json", "-q"])
                .args(&config.args)
                .arg(format!("--stdin-path={}", file.display()))
                .arg("-");
            run(cmd, Some(text), timeout)?
        }
    };
    Ok(match tool {
        Tool::PhpStan => parse_phpstan(&out, file),
        Tool::Psalm => parse_psalm(&out, file),
        Tool::Phpcs => parse_phpcs(&out),
    })
}

fn json_start(out: &str) -> Option<Value> {
    // tools may print notices before the JSON document
    let i = out.find(['{', '['])?;
    serde_json::from_str(&out[i..]).ok()
}

fn same_file(a: &str, b: &Path) -> bool {
    let a = Path::new(a);
    a == b || a.file_name() == b.file_name() && (a.ends_with(b) || b.ends_with(a))
}

pub fn parse_phpstan(out: &str, file: &Path) -> Vec<ToolDiagnostic> {
    let Some(v) = json_start(out) else { return Vec::new() };
    let mut diags = Vec::new();
    if let Some(files) = v.get("files").and_then(Value::as_object) {
        for (path, f) in files {
            if !same_file(path, file) {
                continue;
            }
            for m in f.get("messages").and_then(Value::as_array).into_iter().flatten() {
                let line = m.get("line").and_then(Value::as_u64).unwrap_or(1).max(1) as u32 - 1;
                let mut message = m.get("message").and_then(Value::as_str).unwrap_or("").to_string();
                if let Some(tip) = m.get("tip").and_then(Value::as_str) {
                    message.push_str("\n💡 ");
                    message.push_str(tip);
                }
                diags.push(ToolDiagnostic {
                    line,
                    column: None,
                    end_line: None,
                    end_column: None,
                    message,
                    severity: ToolSeverity::Error,
                    code: m.get("identifier").and_then(Value::as_str).map(str::to_string),
                    source: "phpstan",
                });
            }
        }
    }
    diags
}

pub fn parse_psalm(out: &str, file: &Path) -> Vec<ToolDiagnostic> {
    let Some(Value::Array(items)) = json_start(out) else { return Vec::new() };
    items
        .iter()
        .filter(|i| i.get("file_path").and_then(Value::as_str).is_none_or(|p| same_file(p, file)))
        .map(|i| {
            let num = |k: &str| i.get(k).and_then(Value::as_u64).map(|n| n as u32);
            ToolDiagnostic {
                line: num("line_from").unwrap_or(1).max(1) - 1,
                column: num("column_from").map(|c| c.saturating_sub(1)),
                end_line: num("line_to").map(|l| l.max(1) - 1),
                end_column: num("column_to").map(|c| c.saturating_sub(1)),
                message: i.get("message").and_then(Value::as_str).unwrap_or("").to_string(),
                severity: match i.get("severity").and_then(Value::as_str) {
                    Some("error") => ToolSeverity::Error,
                    Some("info") => ToolSeverity::Information,
                    _ => ToolSeverity::Warning,
                },
                code: i.get("type").and_then(Value::as_str).map(str::to_string),
                source: "psalm",
            }
        })
        .collect()
}

pub fn parse_phpcs(out: &str) -> Vec<ToolDiagnostic> {
    let Some(v) = json_start(out) else { return Vec::new() };
    let mut diags = Vec::new();
    for f in v.get("files").and_then(Value::as_object).into_iter().flat_map(|o| o.values()) {
        for m in f.get("messages").and_then(Value::as_array).into_iter().flatten() {
            let line = m.get("line").and_then(Value::as_u64).unwrap_or(1).max(1) as u32 - 1;
            diags.push(ToolDiagnostic {
                line,
                column: m.get("column").and_then(Value::as_u64).map(|c| (c as u32).saturating_sub(1)),
                end_line: None,
                end_column: None,
                message: m.get("message").and_then(Value::as_str).unwrap_or("").to_string(),
                severity: if m.get("type").and_then(Value::as_str) == Some("ERROR") {
                    ToolSeverity::Error
                } else {
                    ToolSeverity::Warning
                },
                code: m.get("source").and_then(Value::as_str).map(str::to_string),
                source: "phpcs",
            });
        }
    }
    diags
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Formatter {
    PhpCsFixer,
    Phpcbf,
}

/// Formats `text` with an external formatter, returning the new text.
pub fn format(
    formatter: Formatter,
    root: &Path,
    file: &Path,
    text: &str,
    config: &ToolConfig,
) -> Result<String, String> {
    match formatter {
        Formatter::Phpcbf => {
            let exe = find_executable(root, "phpcbf", config.path.as_deref()).ok_or("phpcbf not found")?;
            let mut cmd = command_for(&exe);
            cmd.current_dir(root)
                .arg("-q")
                .args(&config.args)
                .arg(format!("--stdin-path={}", file.display()))
                .arg("-");
            let out = run(cmd, Some(text), Duration::from_secs(30))?;
            if out.trim().is_empty() {
                return Err("phpcbf produced no output".into());
            }
            Ok(out)
        }
        Formatter::PhpCsFixer => {
            let exe = find_executable(root, "php-cs-fixer", config.path.as_deref())
                .ok_or("php-cs-fixer not found")?;
            // php-cs-fixer works on files: format a temporary copy
            let dir = std::env::temp_dir().join(format!("php-lsp-fixer-{}", std::process::id()));
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let tmp = dir.join(file.file_name().unwrap_or_else(|| "file.php".as_ref()));
            std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
            let mut cmd = command_for(&exe);
            cmd.current_dir(root).args(["fix", "--using-cache=no", "--quiet", "--no-interaction"]);
            let config_file = [".php-cs-fixer.php", ".php-cs-fixer.dist.php", ".php_cs", ".php_cs.dist"]
                .iter()
                .map(|c| root.join(c))
                .find(|p| p.exists());
            if let Some(c) = config_file {
                cmd.arg(format!("--config={}", c.display()));
                // config finders would skip files outside the project
                cmd.arg("--path-mode=override");
            }
            cmd.args(&config.args).arg(&tmp);
            cmd.env("PHP_CS_FIXER_IGNORE_ENV", "1");
            run(cmd, None, Duration::from_secs(30))?;
            let out = std::fs::read_to_string(&tmp).map_err(|e| e.to_string());
            let _ = std::fs::remove_file(&tmp);
            out
        }
    }
}

/// Result of running PHPUnit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestRun {
    pub success: bool,
    pub summary: String,
    pub output: String,
}

/// Runs PHPUnit for a file, optionally filtered (`--filter`).
pub fn run_phpunit(
    root: &Path,
    file: &Path,
    filter: Option<&str>,
    config: &ToolConfig,
) -> Result<TestRun, String> {
    let exe = find_executable(root, "phpunit", config.path.as_deref()).ok_or("phpunit not found")?;
    let mut cmd = command_for(&exe);
    cmd.current_dir(root).arg("--colors=never").args(&config.args);
    if let Some(f) = filter {
        cmd.arg("--filter").arg(format!("/{}/", regex_escape(f)));
    }
    cmd.arg(file);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let out = cmd.output().map_err(|e| format!("cannot start phpunit: {e}"))?;
    let output = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let summary = output
        .lines()
        .rev()
        .find(|l| {
            l.starts_with("OK")
                || l.starts_with("Tests:")
                || l.starts_with("FAILURES")
                || l.starts_with("ERRORS")
                || l.contains("No tests executed")
        })
        .unwrap_or("PHPUnit finished")
        .trim()
        .to_string();
    Ok(TestRun { success: out.status.success(), summary, output })
}

fn regex_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if "\\.+*?()|[]{}^$/#".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers() {
        let f = Path::new("/p/src/a.php");
        let stan = r#"{"totals":{"errors":0,"file_errors":1},"files":{"/p/src/a.php":{"errors":1,"messages":[{"message":"Function x not found.","line":6,"ignorable":true,"identifier":"function.notFound"}]}},"errors":[]}"#;
        let d = parse_phpstan(stan, f);
        assert_eq!(d[0].line, 5);
        assert_eq!(d[0].code.as_deref(), Some("function.notFound"));
        let psalm = r#"[{"severity":"error","line_from":3,"line_to":3,"type":"UndefinedFunction","message":"Function x does not exist","file_path":"/p/src/a.php","column_from":5,"column_to":8}]"#;
        let d = parse_psalm(psalm, f);
        assert_eq!((d[0].line, d[0].column, d[0].end_column), (2, Some(4), Some(7)));
        assert_eq!(d[0].severity, ToolSeverity::Error);
        let cs = r#"{"totals":{"errors":1,"warnings":0,"fixable":1},"files":{"\/p\/src\/a.php":{"errors":1,"warnings":0,"messages":[{"message":"Missing blank line","source":"PSR12.Files.FileHeader","severity":5,"fixable":true,"type":"ERROR","line":2,"column":14}]}}}"#;
        let d = parse_phpcs(cs);
        assert_eq!((d[0].line, d[0].column), (1, Some(13)));
        assert_eq!(d[0].code.as_deref(), Some("PSR12.Files.FileHeader"));
    }
}

#[cfg(test)]
mod real_tools {
    //! Runs the real tools when `PHP_LSP_TOOLS_DIR` points to a directory with
    //! `phpstan.phar`, `phpcs.phar`, `phpcbf.phar` and `php-cs-fixer.phar`.
    use super::*;

    #[test]
    #[ignore]
    fn run_real_tools() {
        let dir = PathBuf::from(std::env::var("PHP_LSP_TOOLS_DIR").expect("PHP_LSP_TOOLS_DIR"));
        let root = std::env::temp_dir().join(format!("php-lsp-tools-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src/a.php");
        let text =
            "<?php\nnamespace App;\nclass A {\n  public function f(int $x): string { return $x; }\n}\n";
        std::fs::write(&file, text).unwrap();
        let cfg = |name: &str, args: &[&str]| ToolConfig {
            enabled: true,
            path: Some(dir.join(name).display().to_string()),
            args: args.iter().map(|s| s.to_string()).collect(),
        };
        let stan =
            diagnostics(Tool::PhpStan, &root, &file, text, &cfg("phpstan.phar", &["--level=5"])).unwrap();
        assert!(stan.iter().any(|d| d.message.contains("should return string")), "{stan:?}");
        let cs =
            diagnostics(Tool::Phpcs, &root, &file, text, &cfg("phpcs.phar", &["--standard=PSR12"])).unwrap();
        assert!(!cs.is_empty());
        let fixed = format(Formatter::Phpcbf, &root, &file, text, &cfg("phpcbf.phar", &["--standard=PSR12"]))
            .unwrap();
        assert!(fixed.contains("class A\n{"), "{fixed}");
        let fixed =
            format(Formatter::PhpCsFixer, &root, &file, text, &cfg("php-cs-fixer.phar", &[])).unwrap();
        assert!(fixed.contains("class A\n{"), "{fixed}");
        if dir.join("phpunit.phar").exists() {
            std::fs::create_dir_all(root.join("tests")).unwrap();
            let test = root.join("tests/ATest.php");
            std::fs::write(&test, "<?php\nuse PHPUnit\\Framework\\TestCase;\nclass ATest extends TestCase {\n  public function testOk(): void { $this->assertTrue(true); }\n  public function testFail(): void { $this->assertTrue(false); }\n}\n").unwrap();
            let ok = run_phpunit(&root, &test, Some("ATest::testOk"), &cfg("phpunit.phar", &[])).unwrap();
            assert!(ok.success && ok.summary.starts_with("OK"), "{ok:?}");
            let all = run_phpunit(&root, &test, None, &cfg("phpunit.phar", &[])).unwrap();
            assert!(!all.success && all.summary.contains("Failures: 1"), "{all:?}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
