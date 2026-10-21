//! Laravel: container resolution, Eloquent relations as properties and
//! completion / navigation for config keys, views, routes, translations and
//! environment variables.

use std::path::{Path, PathBuf};

use php_hir::{Fqn, Type};

use super::{
    array_keys, key_for, named_files, returned_array, string_args_of_calls, ArgInfo, Cached, CallInfo,
    Callee, Plugin, PluginCx, StringArg, StringCandidate,
};
use crate::CompletionKind;

const MODEL: &str = "Illuminate\\Database\\Eloquent\\Model";
const RELATION: &str = "Illuminate\\Database\\Eloquent\\Relations\\Relation";
const ELOQUENT_COLLECTION: &str = "Illuminate\\Database\\Eloquent\\Collection";

/// Relations returning several models.
const MANY_RELATIONS: &[&str] =
    &["HasMany", "BelongsToMany", "MorphMany", "MorphToMany", "HasManyThrough", "MorphedByMany"];

/// Container aliases of the most common services.
const ALIASES: &[(&str, &str)] = &[
    ("app", "Illuminate\\Foundation\\Application"),
    ("auth", "Illuminate\\Auth\\AuthManager"),
    ("cache", "Illuminate\\Cache\\CacheManager"),
    ("config", "Illuminate\\Config\\Repository"),
    ("cookie", "Illuminate\\Cookie\\CookieJar"),
    ("db", "Illuminate\\Database\\DatabaseManager"),
    ("events", "Illuminate\\Events\\Dispatcher"),
    ("files", "Illuminate\\Filesystem\\Filesystem"),
    ("filesystem", "Illuminate\\Filesystem\\FilesystemManager"),
    ("hash", "Illuminate\\Hashing\\HashManager"),
    ("log", "Illuminate\\Log\\LogManager"),
    ("mail.manager", "Illuminate\\Mail\\MailManager"),
    ("queue", "Illuminate\\Queue\\QueueManager"),
    ("redirect", "Illuminate\\Routing\\Redirector"),
    ("request", "Illuminate\\Http\\Request"),
    ("router", "Illuminate\\Routing\\Router"),
    ("session", "Illuminate\\Session\\SessionManager"),
    ("translator", "Illuminate\\Translation\\Translator"),
    ("url", "Illuminate\\Routing\\UrlGenerator"),
    ("validator", "Illuminate\\Validation\\Factory"),
    ("view", "Illuminate\\View\\Factory"),
];

#[derive(Default)]
struct ProjectData {
    config: Vec<StringCandidate>,
    views: Vec<StringCandidate>,
    routes: Vec<StringCandidate>,
    translations: Vec<StringCandidate>,
    env: Vec<StringCandidate>,
}

#[derive(Default)]
pub struct Laravel {
    data: Cached<(Option<PathBuf>, ProjectData)>,
}

fn is_laravel_root(root: &Path) -> bool {
    root.join("artisan").exists() || root.join("bootstrap").join("app.php").exists()
}

impl Laravel {
    fn data(&self, cx: &PluginCx) -> std::sync::Arc<(Option<PathBuf>, ProjectData)> {
        let root = cx.project_root().filter(|r| is_laravel_root(r));
        let data = self.data.get(cx.ws.revision(), || (root.clone(), collect(cx, root.as_deref())));
        if data.0 != root {
            // another project in a multi-root workspace
            return std::sync::Arc::new((root.clone(), collect(cx, root.as_deref())));
        }
        data
    }
}

fn candidate(
    value: String,
    detail: &str,
    kind: CompletionKind,
    location: Option<(String, u32)>,
) -> StringCandidate {
    StringCandidate { value, detail: Some(detail.to_string()), kind, location }
}

fn collect(cx: &PluginCx, root: Option<&Path>) -> ProjectData {
    let Some(root) = root else { return ProjectData::default() };
    let mut data = ProjectData::default();
    let ws = cx.ws;
    // config/*.php -> file.key.sub
    let config_prefix = key_for(&root.join("config")) + "/";
    for (id, f) in cx.files_under(&config_prefix) {
        let name = f.path[config_prefix.len()..].trim_end_matches(".php").replace('/', ".");
        let Some(parse) = ws.parse(id) else { continue };
        let root_node = parse.syntax_node();
        data.config.push(candidate(
            name.clone(),
            "config file",
            CompletionKind::File,
            Some((f.path.to_string(), 0)),
        ));
        if let Some(array) = returned_array(&root_node) {
            let mut keys = Vec::new();
            array_keys(&array, &format!("{name}."), &mut keys);
            for (k, range) in keys {
                let line = f.line_index.line_col(range.start()).line;
                data.config.push(candidate(
                    k,
                    "config",
                    CompletionKind::Property,
                    Some((f.path.to_string(), line)),
                ));
            }
        }
    }
    // lang/{locale}/*.php (or resources/lang)
    for lang_dir in [root.join("lang"), root.join("resources").join("lang")] {
        let Ok(locales) = std::fs::read_dir(&lang_dir) else { continue };
        let mut seen = std::collections::HashSet::new();
        for locale in locales.flatten().filter(|e| e.path().is_dir()) {
            let prefix = key_for(&locale.path()) + "/";
            for (id, f) in cx.files_under(&prefix) {
                let file = f.path[prefix.len()..].trim_end_matches(".php").replace('/', ".");
                let Some(parse) = ws.parse(id) else { continue };
                if let Some(array) = returned_array(&parse.syntax_node()) {
                    let mut keys = Vec::new();
                    array_keys(&array, &format!("{file}."), &mut keys);
                    for (k, range) in keys {
                        if seen.insert(k.clone()) {
                            let line = f.line_index.line_col(range.start()).line;
                            data.translations.push(candidate(
                                k,
                                "translation",
                                CompletionKind::Property,
                                Some((f.path.to_string(), line)),
                            ));
                        }
                    }
                }
            }
        }
    }
    // resources/views/**/*.blade.php -> dot names
    for (name, path) in named_files(&root.join("resources").join("views"), ".blade.php", ".") {
        data.views.push(candidate(name, "view", CompletionKind::File, Some((key_for(&path), 0))));
    }
    for (name, path) in named_files(&root.join("resources").join("views"), ".php", ".") {
        if !path.to_string_lossy().ends_with(".blade.php") {
            data.views.push(candidate(name, "view", CompletionKind::File, Some((key_for(&path), 0))));
        }
    }
    // routes/*.php: ->name('x')
    let routes_prefix = key_for(&root.join("routes")) + "/";
    for (id, f) in cx.files_under(&routes_prefix) {
        let Some(parse) = ws.parse(id) else { continue };
        for (name, range) in string_args_of_calls(&parse.syntax_node(), &["name"], 0) {
            let line = f.line_index.line_col(range.start()).line;
            data.routes.push(candidate(
                name,
                "route",
                CompletionKind::Constant,
                Some((f.path.to_string(), line)),
            ));
        }
    }
    // .env keys
    if let Ok(env) = std::fs::read_to_string(root.join(".env"))
        .or_else(|_| std::fs::read_to_string(root.join(".env.example")))
    {
        let path = if root.join(".env").exists() { root.join(".env") } else { root.join(".env.example") };
        for (i, line) in env.lines().enumerate() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                let k = k.trim().trim_start_matches("export ").to_string();
                if !k.is_empty() {
                    data.env.push(StringCandidate {
                        value: k,
                        detail: Some(v.trim().chars().take(40).collect()),
                        kind: CompletionKind::Variable,
                        location: Some((key_for(&path), i as u32)),
                    });
                }
            }
        }
    }
    data
}

impl Laravel {
    fn container_type(&self, cx: &PluginCx, call: &CallInfo) -> Option<Type> {
        let first = call.args.first();
        let resolved = |arg: Option<&ArgInfo>| -> Option<Type> {
            match arg? {
                ArgInfo::ClassString(c) => Some(Type::Named(c.clone(), Vec::new())),
                ArgInfo::String(s) => {
                    let class = ALIASES
                        .iter()
                        .find(|(a, _)| a == s)
                        .map(|(_, c)| Fqn::new(c))
                        .unwrap_or_else(|| Fqn::new(s));
                    cx.ws.index().class(&class).map(|_| Type::Named(class, Vec::new()))
                }
                ArgInfo::Other => None,
            }
        };
        match &call.callee {
            c if c.is_function("app") || c.is_function("resolve") => {
                if call.args.is_empty() {
                    return Some(Type::named("Illuminate\\Foundation\\Application"));
                }
                resolved(first)
            }
            Callee::Method { class, name, .. }
                if matches!(name.to_ascii_lowercase().as_str(), "make" | "makewith" | "get" | "resolve") =>
            {
                let is_container = cx.is_subtype(class, "Illuminate\\Contracts\\Container\\Container")
                    || class.short() == "App" && class.as_str().contains("Facades");
                if is_container {
                    resolved(first)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl Plugin for Laravel {
    fn name(&self) -> &'static str {
        "laravel"
    }

    fn call_return_type(&self, cx: &PluginCx, call: &CallInfo) -> Option<Type> {
        if let Some(t) = self.container_type(cx, call) {
            return Some(t);
        }
        // relation methods: `$this->hasMany(Post::class)` without generics
        if let Callee::Method { class, name, .. } = &call.callee {
            let lower = name.to_ascii_lowercase();
            if cx.is_subtype(class, MODEL)
                && matches!(
                    lower.as_str(),
                    "hasmany"
                        | "hasone"
                        | "belongsto"
                        | "belongstomany"
                        | "morphmany"
                        | "morphone"
                        | "morphtomany"
                        | "hasmanythrough"
                        | "hasonethrough"
                )
            {
                if let Some(ArgInfo::ClassString(related)) = call.args.first() {
                    let rel = match lower.as_str() {
                        "hasmany" => "HasMany",
                        "hasone" => "HasOne",
                        "belongsto" => "BelongsTo",
                        "belongstomany" => "BelongsToMany",
                        "morphmany" => "MorphMany",
                        "morphone" => "MorphOne",
                        "morphtomany" => "MorphToMany",
                        "hasmanythrough" => "HasManyThrough",
                        _ => "HasOneThrough",
                    };
                    return Some(Type::Named(
                        Fqn::new(&format!("Illuminate\\Database\\Eloquent\\Relations\\{rel}")),
                        vec![Type::Named(related.clone(), Vec::new())],
                    ));
                }
            }
        }
        None
    }

    fn property_type(&self, cx: &PluginCx, class: &Fqn, name: &str) -> Option<Type> {
        if !cx.is_subtype(class, MODEL) {
            return None;
        }
        // `$user->posts` -> result of the `posts()` relation
        let m = cx.ws.index().find_method(class, name)?;
        let ret = m.def.effective_return_type()?;
        let (rel_class, args) = ret.members().into_iter().find_map(|t| match t {
            Type::Named(n, args) if cx.is_subtype(n, RELATION) => Some((n.clone(), args.clone())),
            _ => None,
        })?;
        let related = match args.first() {
            Some(t) => t.clone(),
            None => {
                // infer from the method body: `return $this->hasMany(Post::class);`
                let (file, _) = (m.file, ());
                let text = cx.ws.file(file)?.text.clone();
                let body = &text[m.def.range];
                let i = body.find("::class")?;
                let start = body[..i].rfind(['(', ' ', ',']).map_or(0, |s| s + 1);
                let name = body[start..i].trim();
                let parse = cx.ws.parse(file)?;
                let scopes = php_hir::FileScopes::build(&parse.tree());
                Type::Named(scopes.at(m.def.range.start()).resolve_class(name), Vec::new())
            }
        };
        if MANY_RELATIONS.iter().any(|r| rel_class.short() == *r) {
            Some(Type::Named(Fqn::new(ELOQUENT_COLLECTION), vec![Type::Int, related]))
        } else {
            Some(related.nullable())
        }
    }

    fn string_candidates(&self, cx: &PluginCx, arg: &StringArg) -> Vec<StringCandidate> {
        let c = &arg.callee;
        let method = c.method_name().map(str::to_ascii_lowercase);
        let class_short = match c {
            Callee::Method { class, .. } => class.short().to_ascii_lowercase(),
            _ => String::new(),
        };
        let data = self.data(cx);
        let d = &data.1;
        let pick = |v: &Vec<StringCandidate>| v.clone();
        if arg.index == 0 {
            if c.is_function("config")
                || class_short == "config"
                || class_short == "repository" && method.as_deref() == Some("get")
            {
                return pick(&d.config);
            }
            if c.is_function("view")
                || class_short == "view" && method.as_deref() == Some("make")
                || method.as_deref() == Some("view") && class_short != "route"
            {
                return pick(&d.views);
            }
            if c.is_function("route")
                || c.is_function("to_route")
                || method.as_deref() == Some("route") && class_short != "route"
                || class_short == "url" && method.as_deref() == Some("route")
            {
                return pick(&d.routes);
            }
            if c.is_function("__")
                || c.is_function("trans")
                || c.is_function("trans_choice")
                || class_short == "lang" && method.as_deref() == Some("get")
            {
                return pick(&d.translations);
            }
            if c.is_function("env") {
                return pick(&d.env);
            }
        }
        // Route::view('/uri', 'view.name')
        if arg.index == 1 && class_short == "route" && method.as_deref() == Some("view") {
            return pick(&d.views);
        }
        Vec::new()
    }
}
