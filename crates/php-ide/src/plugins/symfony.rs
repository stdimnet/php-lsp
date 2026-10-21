//! Symfony / Doctrine: typed container and repository access, completion
//! and navigation for route names and Twig templates.

use std::path::PathBuf;

use php_hir::{Fqn, Type};
use php_syntax::SyntaxKind::*;

use super::{
    key_for, named_files, unquote, ArgInfo, Cached, CallInfo, Callee, Plugin, PluginCx, StringArg,
    StringCandidate,
};
use crate::CompletionKind;

#[derive(Default)]
struct ProjectData {
    routes: Vec<StringCandidate>,
    templates: Vec<StringCandidate>,
}

#[derive(Default)]
pub struct Symfony {
    data: Cached<(Option<PathBuf>, ProjectData)>,
}

const CONTAINERS: &[&str] =
    &["Psr\\Container\\ContainerInterface", "Symfony\\Component\\DependencyInjection\\ContainerInterface"];
const MANAGERS: &[&str] = &[
    "Doctrine\\Persistence\\ObjectManager",
    "Doctrine\\Persistence\\ManagerRegistry",
    "Doctrine\\ORM\\EntityManagerInterface",
];

fn collect(cx: &PluginCx, root: Option<&std::path::Path>) -> ProjectData {
    let Some(root) = root else { return ProjectData::default() };
    let mut data = ProjectData::default();
    // #[Route(..., name: 'x')] in workspace PHP files
    let prefix = key_for(root) + "/";
    for (id, f) in cx
        .files_under(&prefix)
        .filter(|(_, f)| f.kind == php_index::FileKind::Workspace && f.text.contains("Route("))
    {
        let Some(parse) = cx.ws.parse(id) else { continue };
        for attr in parse.syntax_node().descendants().filter(|n| n.kind() == ATTRIBUTE) {
            let is_route = attr.children().find(|c| c.kind() == QUALIFIED_NAME).is_some_and(|q| {
                php_syntax::ast::text_without_trivia(&q).rsplit('\\').next() == Some("Route")
            });
            if !is_route {
                continue;
            }
            for arg in attr.descendants().filter(|n| n.kind() == ARG) {
                let named = arg
                    .children()
                    .find(|c| c.kind() == NAME)
                    .map(|n| php_syntax::ast::text_without_trivia(&n));
                if named.as_deref() != Some("name") {
                    continue;
                }
                let Some(tok) =
                    arg.descendants_with_tokens().filter_map(|e| e.into_token()).find(|t| t.kind() == STRING)
                else {
                    continue;
                };
                let line = f.line_index.line_col(tok.text_range().start()).line;
                data.routes.push(StringCandidate {
                    value: unquote(tok.text()),
                    detail: Some("route".into()),
                    kind: CompletionKind::Constant,
                    location: Some((f.path.to_string(), line)),
                });
            }
        }
    }
    // config/routes*.yaml: top level keys are route names
    for dir in [root.join("config").join("routes"), root.join("config")] {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !(name.ends_with(".yaml") || name.ends_with(".yml"))
                || dir.ends_with("config") && !name.starts_with("routes")
            {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            for (i, line) in text.lines().enumerate() {
                if line.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
                    && line.trim_end().ends_with(':')
                    && !line.contains(' ')
                {
                    let key = line.trim_end().trim_end_matches(':').to_string();
                    if key != "when@dev" && !key.starts_with("when@") {
                        data.routes.push(StringCandidate {
                            value: key,
                            detail: Some("route".into()),
                            kind: CompletionKind::Constant,
                            location: Some((key_for(&p), i as u32)),
                        });
                    }
                }
            }
        }
    }
    for (name, path) in named_files(&root.join("templates"), ".twig", "/") {
        data.templates.push(StringCandidate {
            value: format!("{name}.twig"),
            detail: Some("template".into()),
            kind: CompletionKind::File,
            location: Some((key_for(&path), 0)),
        });
    }
    data
}

impl Plugin for Symfony {
    fn name(&self) -> &'static str {
        "symfony"
    }

    fn call_return_type(&self, cx: &PluginCx, call: &CallInfo) -> Option<Type> {
        let Callee::Method { class, name, .. } = &call.callee else { return None };
        let Some(ArgInfo::ClassString(target)) = call.args.first() else { return None };
        let name = name.to_ascii_lowercase();
        if name == "get" && CONTAINERS.iter().any(|c| cx.is_subtype(class, c)) {
            return Some(Type::Named(target.clone(), Vec::new()));
        }
        if name == "getrepository"
            && (MANAGERS.iter().any(|c| cx.is_subtype(class, c)) || class.short().contains("EntityManager"))
        {
            // #[ORM\Entity(repositoryClass: FooRepository::class)]
            if let Some((file, def)) = cx.ws.index().class(target) {
                if let Some(f) = cx.ws.file(file) {
                    let start = usize::from(def.range.start());
                    let header_start =
                        f.text[..start].rfind("#[").filter(|i| start - i < 400).unwrap_or(start);
                    let header = &f.text[header_start..start];
                    if let Some(i) = header.find("repositoryClass") {
                        let rest = &header[i..];
                        if let (Some(colon), Some(end)) = (rest.find([':', '=']), rest.find("::class")) {
                            let name = rest[colon + 1..end].trim().trim_start_matches('>').trim();
                            if let Some(parse) = cx.ws.parse(file) {
                                let scopes = php_hir::FileScopes::build(&parse.tree());
                                let repo = scopes.at(def.range.start()).resolve_class(name);
                                return Some(Type::Named(repo, Vec::new()));
                            }
                        }
                    }
                }
            }
            return Some(Type::Named(
                Fqn::new("Doctrine\\ORM\\EntityRepository"),
                vec![Type::Named(target.clone(), Vec::new())],
            ));
        }
        None
    }

    fn string_candidates(&self, cx: &PluginCx, arg: &StringArg) -> Vec<StringCandidate> {
        let Some(method) = arg.callee.method_name().map(str::to_ascii_lowercase) else { return Vec::new() };
        if arg.index != 0 {
            return Vec::new();
        }
        let root = cx
            .project_root()
            .filter(|r| r.join("symfony.lock").exists() || r.join("bin").join("console").exists());
        let data = self.data.get(cx.ws.revision(), || (root.clone(), collect(cx, root.as_deref())));
        match method.as_str() {
            "generateurl" | "redirecttoroute" | "generate" => data.1.routes.clone(),
            "render" | "renderview" | "renderblock" | "renderblockview" | "stream" => {
                data.1.templates.clone()
            }
            _ => Vec::new(),
        }
    }
}
