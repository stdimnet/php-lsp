//! A framework plugin: `container('mailer')` returns a `Mailer`, and the
//! service names are completed inside the string argument.
//! Run: `cargo run -p php-ide --example plugin`

use std::sync::Arc;

use php_ide::hir::{Fqn, Type};
use php_ide::plugins::{ArgInfo, CallInfo, Plugin, PluginCx, StringArg, StringCandidate};
use php_ide::{CompletionKind, Config, LineCol, Workspace};

struct Services;

const SERVICES: &[(&str, &str)] = &[("mailer", "App\\Mailer"), ("logger", "App\\Logger")];

impl Plugin for Services {
    fn name(&self) -> &'static str {
        "services"
    }

    fn call_return_type(&self, _cx: &PluginCx, call: &CallInfo) -> Option<Type> {
        if !call.callee.is_function("container") {
            return None;
        }
        let ArgInfo::String(name) = call.args.first()? else { return None };
        let class = SERVICES.iter().find(|(n, _)| n == name)?.1;
        Some(Type::Named(Fqn::new(class), Vec::new()))
    }

    fn string_candidates(&self, _cx: &PluginCx, arg: &StringArg) -> Vec<StringCandidate> {
        if !arg.callee.is_function("container") || arg.index != 0 {
            return Vec::new();
        }
        SERVICES
            .iter()
            .map(|(name, class)| StringCandidate {
                value: name.to_string(),
                detail: Some(class.to_string()),
                kind: CompletionKind::Constant,
                location: None,
            })
            .collect()
    }
}

fn main() {
    let mut ws = Workspace::new(Config::default());
    ws.register_plugin(Arc::new(Services));
    ws.index_files(vec![(
        "file:///app/Mailer.php".into(),
        "<?php\nnamespace App;\nclass Mailer { public function send(string $to): void {} }\n".into(),
        php_ide::FileKind::Workspace,
    )]);
    let text = "<?php\ncontainer('mailer')->send('a@b.c');\ncontainer('');\n";
    let id = ws.set_file("file:///app/index.php", text.to_string());
    let lines = ws.line_index(id).expect("open file");
    let at = |line, col| lines.offset(LineCol { line, col });
    let members: Vec<String> = ws.completions(id, at(1, 21)).items.into_iter().map(|i| i.label).collect();
    println!("members of container('mailer'): {members:?}");
    let names: Vec<String> = ws.completions(id, at(2, 11)).items.into_iter().map(|i| i.label).collect();
    println!("service names: {names:?}");
}
