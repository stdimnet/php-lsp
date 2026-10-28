//! Feature tests using fixtures with a `<|>` cursor marker. Multiple files are
//! separated by `//- name.php` lines.

use php_ide::{Config, FileId, TextSize, Workspace};

struct Fixture {
    ws: Workspace,
    file: FileId,
    offset: TextSize,
}

fn fixture(text: &str) -> Fixture {
    fixture_with(text, false)
}

fn fixture_with(text: &str, stubs: bool) -> Fixture {
    let mut ws = Workspace::new(Config::default());
    if stubs {
        ws.load_stubs();
    }
    let mut cursor = None;
    let mut files: Vec<(String, String)> = Vec::new();
    if text.trim_start().starts_with("//-") {
        for chunk in text.split("//- ").filter(|c| !c.trim().is_empty()) {
            let (name, body) = chunk.split_once('\n').unwrap();
            files.push((name.trim().to_string(), body.to_string()));
        }
    } else {
        files.push(("main.php".to_string(), text.to_string()));
    }
    for (name, body) in files {
        let path = format!("file:///{name}");
        if let Some(i) = body.find("<|>") {
            let clean = body.replacen("<|>", "", 1);
            let id = ws.set_file(&path, clean);
            cursor = Some((id, TextSize::from(i as u32)));
        } else {
            ws.set_file(&path, body);
        }
    }
    let (file, offset) = cursor.expect("fixture needs a <|> cursor");
    Fixture { ws, file, offset }
}

fn hover(text: &str) -> String {
    let f = fixture(text);
    f.ws.hover(f.file, f.offset).map(|h| h.markdown).unwrap_or_default()
}

fn type_at(text: &str) -> String {
    let f = fixture(text);
    f.ws.type_at(f.file, f.offset).unwrap_or_default()
}

fn goto(text: &str) -> Vec<String> {
    let f = fixture(text);
    f.ws.goto_definition(f.file, f.offset)
        .into_iter()
        .map(|l| {
            let file = f.ws.file(l.file).unwrap();
            format!("{}:{}", file.path.trim_start_matches("file:///"), &file.text[l.focus])
        })
        .collect()
}

fn completion_labels(text: &str) -> Vec<String> {
    let f = fixture(text);
    f.ws.completions(f.file, f.offset).items.into_iter().map(|c| c.label).collect()
}

const MODELS: &str = r#"<?php
namespace App\Models;

/**
 * A blog post.
 * @property-read int $id
 */
class Post {
    public string $title = '';
    protected array $tags = [];
    private int $secret = 0;
    public const STATUS = 'draft';
    public function author(): User { return new User(); }
    public static function find(int $id): ?static { return null; }
    /** @return Post[] */
    public static function all(): array { return []; }
    private function hidden() {}
}

class User {
    public function __construct(public string $name = '', private ?Post $latest = null) {}
    /** Returns the posts. */
    public function posts(): array { return []; }
    public function latest(): ?Post { return $this->latest; }
}
"#;

#[test]
fn infers_variable_and_call_types() {
    let text = format!(
        "//- models.php\n{MODELS}//- main.php\n<?php\nuse App\\Models\\Post;\n$p = Post::find(1);\n$u = $p->author();\n$x = $u<|>->latest();\n"
    );
    assert_eq!(type_at(&text), "App\\Models\\User");
    let text = format!(
        "//- models.php\n{MODELS}//- main.php\n<?php\nuse App\\Models\\Post;\n$p = Post::find(1);\n$p<|>;\n"
    );
    assert_eq!(type_at(&text), "?App\\Models\\Post");
    let text = format!(
        "//- models.php\n{MODELS}//- main.php\n<?php\nuse App\\Models\\Post;\nforeach (Post::all() as $post) {{ $post<|>; }}\n"
    );
    assert_eq!(type_at(&text), "App\\Models\\Post");
    let text = format!(
        "//- models.php\n{MODELS}//- main.php\n<?php\nfunction f($x) {{ if ($x instanceof \\App\\Models\\User) {{ $x<|>; }} }}\n"
    );
    assert_eq!(type_at(&text), "App\\Models\\User");
    let text = "<?php\n/** @var \\DateTime $d */\n$d<|>;\n";
    assert_eq!(type_at(text), "DateTime");
    let text = "<?php\n$a = ['name' => 'x', 'age' => 1];\n$a['age']<|>;\n";
    assert_eq!(type_at(text), "int");
    let text = "<?php\nfunction f(int ...$nums) { $nums<|>; }\n";
    assert_eq!(type_at(text), "list<int>");
    let text = "<?php\n$f = fn(int $x): string => 'a';\n$r = $f(1);\n$r<|>;\n";
    assert_eq!(type_at(text), "string");
}

#[test]
fn hover_shows_signature_and_doc() {
    let text = format!(
        "//- models.php\n{MODELS}//- main.php\n<?php\n$u = new \\App\\Models\\User();\n$u->po<|>sts();\n"
    );
    let h = hover(&text);
    assert!(h.contains("public function posts(): array { }"), "{h}");
    assert!(h.contains("Returns the posts."), "{h}");
    let text =
        format!("//- models.php\n{MODELS}//- main.php\n<?php\nuse App\\Models\\Post;\nPo<|>st::find(1);\n");
    let h = hover(&text);
    assert!(h.contains("namespace App\\Models;"), "{h}");
    assert!(h.contains("class Post { }"), "{h}");
    assert!(h.contains("A blog post."), "{h}");
    let h = hover("<?php\nfunction f(string $name) { $na<|>me; }\n");
    assert!(h.contains("string $name"), "{h}");
}

#[test]
fn goto_definition_variants() {
    let text =
        format!("//- models.php\n{MODELS}//- main.php\n<?php\nuse App\\Models\\Post;\n$p = new Po<|>st();\n");
    assert_eq!(goto(&text), vec!["models.php:Post"]);
    let text = format!("//- models.php\n{MODELS}//- main.php\n<?php\n(new \\App\\Models\\User)->name<|>;\n");
    assert_eq!(goto(&text), vec!["models.php:$name"]);
    let text = format!("//- models.php\n{MODELS}//- main.php\n<?php\n\\App\\Models\\Post::STAT<|>US;\n");
    assert_eq!(goto(&text), vec!["models.php:STATUS"]);
    let text = "<?php\nfunction f() { $a = 1; echo $a<|>; }\n";
    assert_eq!(goto(text), vec!["main.php:$a"]);
    let text = "<?php\nnamespace N;\nfunction helper() {}\nhel<|>per();\n";
    assert_eq!(goto(text), vec!["main.php:helper"]);
    let text = "<?php\nclass A { function m() { $this->n<|>(); } function n() {} }\n";
    assert_eq!(goto(text), vec!["main.php:n"]);
    let text = "<?php\nclass P { function m() {} }\nclass C extends P { function x() { parent::m<|>(); } }\n";
    assert_eq!(goto(text), vec!["main.php:m"]);
    let text =
        "<?php\n$f = function () use ($v) {};\n$v = 1;\n$g = function () use ($v) { return $v<|>; };\n";
    assert_eq!(goto(text), vec!["main.php:$v"]);
}

#[test]
fn references_across_files() {
    let text = format!(
        "//- models.php\n{MODELS}//- a.php\n<?php\nuse App\\Models\\User;\nfunction a(User $u) {{ return $u->pos<|>ts(); }}\n//- b.php\n<?php\n$x = new \\App\\Models\\User();\n$x->posts();\n$x->latest();\n"
    );
    let f = fixture(&text);
    let refs = f.ws.references(f.file, f.offset, true);
    let files: Vec<String> = refs
        .iter()
        .map(|r| {
            let file = f.ws.file(r.file).unwrap();
            format!("{}:{}", file.path.trim_start_matches("file:///"), &file.text[r.range])
        })
        .collect();
    assert_eq!(files, vec!["models.php:posts", "a.php:posts", "b.php:posts"]);
    let refs = f.ws.references(f.file, f.offset, false);
    assert_eq!(refs.len(), 2);
}

#[test]
fn references_of_class_and_variable() {
    let text =
        "<?php\nclass Foo {}\nfunction f(Foo $a): Foo { $b = new Foo(); return $b; }\n$x = Fo<|>o::class;\n";
    let f = fixture(text);
    assert_eq!(f.ws.references(f.file, f.offset, true).len(), 5);
    let text = "<?php\nfunction f() { $a = 1; $a++; return $a<|>; }\nfunction g() { $a = 2; }\n";
    let f = fixture(text);
    let hl = f.ws.document_highlights(f.file, f.offset);
    assert_eq!(hl.len(), 3);
    assert_eq!(hl[0].kind, php_ide::ReferenceKind::Write);
}

#[test]
fn member_completion_respects_visibility() {
    let text =
        format!("//- models.php\n{MODELS}//- main.php\n<?php\n$p = new \\App\\Models\\Post();\n$p-><|>\n");
    let labels = completion_labels(&text);
    assert!(labels.contains(&"title".to_string()), "{labels:?}");
    assert!(labels.contains(&"author".to_string()));
    assert!(labels.contains(&"id".to_string()), "magic @property");
    assert!(!labels.contains(&"secret".to_string()), "private property");
    assert!(!labels.contains(&"tags".to_string()), "protected property");
    assert!(!labels.contains(&"hidden".to_string()), "private method");

    let text = format!("//- models.php\n{MODELS}//- main.php\n<?php\n\\App\\Models\\Post::<|>\n");
    let labels = completion_labels(&text);
    assert!(labels.contains(&"STATUS".to_string()), "{labels:?}");
    assert!(labels.contains(&"find".to_string()));
    assert!(labels.contains(&"class".to_string()));
    assert!(!labels.contains(&"author".to_string()), "instance method via ::");

    let text = "<?php\nclass A { private $x; private function p() {} function f() { $this-><|> } }\n";
    let labels = completion_labels(text);
    assert!(labels.contains(&"x".to_string()) && labels.contains(&"p".to_string()), "{labels:?}");
}

#[test]
fn class_completion_with_auto_import() {
    let text = format!("//- models.php\n{MODELS}//- main.php\n<?php\nnamespace App\\Http;\n\nnew Pos<|>\n");
    let f = fixture(&text);
    let list = f.ws.completions(f.file, f.offset);
    let item = list.items.iter().find(|i| i.label == "Post").expect("Post completion");
    assert_eq!(item.insert_text, "Post($0)");
    assert_eq!(item.additional_edits.len(), 1);
    assert_eq!(item.additional_edits[0].new_text, "\n\nuse App\\Models\\Post;");
    // already imported: no edit
    let text = format!("//- models.php\n{MODELS}//- main.php\n<?php\nuse App\\Models\\Post;\n$x = Pos<|>\n");
    let f = fixture(&text);
    let list = f.ws.completions(f.file, f.offset);
    let item = list.items.iter().find(|i| i.label == "Post").unwrap();
    assert!(item.additional_edits.is_empty());
}

#[test]
fn variable_and_keyword_completion() {
    let labels = completion_labels("<?php\nfunction f($alpha) { $beta = 1; $<|> }\n");
    assert!(labels.contains(&"$alpha".to_string()) && labels.contains(&"$beta".to_string()), "{labels:?}");
    let labels = completion_labels("<?php\nfore<|>\n");
    assert!(labels.contains(&"foreach".to_string()), "{labels:?}");
    let labels = completion_labels("<?php\n/**\n * @par<|>\n */\nfunction f() {}\n");
    assert!(labels.contains(&"param".to_string()), "{labels:?}");
}

#[test]
fn signature_help_tracks_active_parameter() {
    let f = fixture(
        "<?php\n/** Adds. */\nfunction add(int $a, int $b = 2): int { return $a + $b; }\nadd(1, <|>);\n",
    );
    let sh = f.ws.signature_help(f.file, f.offset).unwrap();
    assert_eq!(sh.signatures[0].label, "add(int $a, int $b = 2): int");
    assert_eq!(sh.active_parameter, Some(1));
    let f = fixture("<?php\nclass P { function __construct(string $name) {} }\nnew P(<|>\n");
    let sh = f.ws.signature_help(f.file, f.offset).unwrap();
    assert_eq!(sh.signatures[0].label, "__construct(string $name)");
}

#[test]
fn stubs_are_indexed() {
    let f = fixture_with("<?php\n$len = strl<|>en('abc');\n", true);
    let h = f.ws.hover(f.file, f.offset).unwrap().markdown;
    assert!(h.contains("function strlen(string $string): int"), "{h}");
    let f = fixture_with("<?php\n$d = new DateTime();\n$d-><|>\n", true);
    let labels: Vec<String> = f.ws.completions(f.file, f.offset).items.into_iter().map(|i| i.label).collect();
    assert!(labels.contains(&"format".to_string()), "{labels:?}");
    let syms = f.ws.workspace_symbols("DateTimeImm", 10);
    assert_eq!(syms[0].name, "DateTimeImmutable");
}

/// phpstorm-stubs' `meta` attributes are loaded by default (issue #2).
#[test]
fn jetbrains_attributes_are_known() {
    let text = "<?php\nuse JetBrains\\PhpStorm\\Pure;\nuse JetBrains\\PhpStorm\\Deprecated;\nfinal class Money\n{\n    #[Pure]\n    public function cents(): int { return 1; }\n    #[Deprecated(reason: 'Use cents()')]\n    public function amount(): int { return 1; }\n}\n(new Money())->amount();\n";
    assert_eq!(diags(text), vec!["P1007 'Money::amount' is deprecated. Use cents()"]);
}

fn diags(text: &str) -> Vec<String> {
    let mut ws = Workspace::new(Config::default());
    ws.load_stubs();
    let id = ws.set_file("file:///d.php", text.to_string());
    ws.diagnostics(id).into_iter().map(|d| format!("{} {}", d.code.as_str(), d.message)).collect()
}

#[test]
fn semantic_diagnostics() {
    let d = diags(
        r#"<?php
namespace App;
use Some\Unused;
interface Shape { public function area(): float; }
abstract class Base { abstract function name(): string; }
class Circle extends Base implements Shape {}
class Ok extends Base implements Shape { function area(): float { return 1.0; } function name(): string { return ''; } }
function two(int $a, int $b) {}
function f(Ok $ok) {
    two(1);
    two(1, 2, 3);
    $ok->missing();
    $ok->area();
    echo $ok->nope;
    echo Ok::NOPE;
    new Shape();
    new Nope();
    undefined_fn();
    echo $undefinedVar;
    $unused = 1;
    if (isset($maybe)) {}
    foreach ([1] as $item) { echo $item; }
    preg_match('/x/', 'x', $m);
    echo $m[0];
}
"#,
    );
    let expected = [
        "P1003 Symbol 'Some\\Unused' is declared but not used.",
        "P1037 'App\\Circle' does not implement methods App\\Base::name, App\\Shape::area.",
        "P1005 Not enough arguments. Expected 2. Found 1.",
        "P1119 Too many arguments. Expected 2. Found 3.",
        "P1013 Undefined method 'missing'.",
        "P1014 Undefined property '$nope'.",
        "P1012 Undefined class constant 'NOPE'.",
        "P1098 Cannot instantiate interface App\\Shape.",
        "P1009 Undefined type 'App\\Nope'.",
        "P1010 Undefined function 'App\\undefined_fn'.",
        "P1008 Undefined variable '$undefinedVar'.",
        "P1003 Symbol '$unused' is declared but not used.",
        // unused parameters of `two()`
        "P1003 Symbol '$a' is declared but not used.",
        "P1003 Symbol '$b' is declared but not used.",
    ];
    for e in expected {
        assert!(d.iter().any(|x| x == e), "missing `{e}` in {d:#?}");
    }
    assert_eq!(d.len(), expected.len(), "{d:#?}");
}

#[test]
fn no_false_positives_for_dynamic_code() {
    let d = diags(
        r#"<?php
class Magic { function __call($n, $a) {} function __get($n) {} }
#[\AllowDynamicProperties] class Dyn {}
trait T { function t() { return $this->fromUser(); } }
function g(Magic $m, Dyn $d, \stdClass $s) {
    $m->anything();
    echo $m->prop;
    echo $d->x;
    echo $s->y;
    $x = $y ?? 1;
    echo @$z;
    $cb = function () use (&$count) { $count++; };
    $cb();
    while (true) { if ($first ?? false) {} $later = 1; echo $later; }
    echo $x;
}
"#,
    );
    // untyped magic method parameters only get the P1132 hint
    let d: Vec<_> = d.into_iter().filter(|d| !d.starts_with("P1132")).collect();
    assert!(d.is_empty(), "{d:#?}");
}

#[test]
fn inlay_hints_and_semantic_tokens() {
    let text = "<?php\nfunction greet(string $name, int $times = 1) {}\n$name = 'x';\ngreet($name, 3);\ngreet('bob');\n$f = fn($x) => 'a' . $x;\n$g = function () { return 1; };\n";
    let mut ws = Workspace::new(Config::default());
    let id = ws.set_file("file:///h.php", text.to_string());
    let hints: Vec<String> = ws
        .inlay_hints(id, php_ide::TextRange::up_to(TextSize::from(text.len() as u32)))
        .into_iter()
        .map(|h| format!("{}@{}", h.label, &text[usize::from(h.offset)..usize::from(h.offset) + 3]))
        .collect();
    assert_eq!(
        hints,
        vec![": void@ {}", "times:@3);", "name:@'bo", ": string@ =>", ": int@ { "],
        "{hints:?}"
    );

    let text = "<?php\nclass A { const C = 1; public static function make(): static { return new static(); } }\n$a = A::make();\necho A::C;\n";
    let id = ws.set_file("file:///s.php", text.to_string());
    let toks: Vec<String> = ws
        .semantic_tokens(id, None)
        .into_iter()
        .map(|t| format!("{}:{:?}:{}", &text[t.range], t.token_type, t.modifiers))
        .collect();
    assert_eq!(
        toks,
        vec![
            "A:Class:1",
            "C:Property:7",
            "make:Method:3",
            "$a:Variable:0",
            "A:Class:0",
            "make:Method:2",
            "A:Class:0",
            "C:Property:6"
        ],
        "{toks:?}"
    );
}

fn apply(text: &str, change: &php_ide::SourceChange, file: FileId) -> String {
    let mut edits: Vec<_> =
        change.edits.iter().filter(|(f, _)| *f == file).flat_map(|(_, e)| e.clone()).collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
    let mut out = text.to_string();
    for e in edits {
        out.replace_range(usize::from(e.range.start())..usize::from(e.range.end()), &e.new_text);
    }
    out
}

#[test]
fn rename_symbols() {
    let text = "<?php\nclass Foo { public int $count = 0; function inc() { $this->count++; return $this->count; } }\n$f = new Foo();\necho $f->count;\n";
    let mut ws = Workspace::new(Config::default());
    let id = ws.set_file("file:///r.php", text.to_string());
    let at = TextSize::from(text.find("$count").unwrap() as u32 + 1);
    let change = ws.rename(id, at, "total").unwrap();
    assert_eq!(
        apply(text, &change, id),
        "<?php\nclass Foo { public int $total = 0; function inc() { $this->total++; return $this->total; } }\n$f = new Foo();\necho $f->total;\n"
    );
    let at = TextSize::from(text.find("Foo()").unwrap() as u32);
    let change = ws.rename(id, at, "Bar").unwrap();
    assert!(
        apply(text, &change, id).contains("class Bar {") && apply(text, &change, id).contains("new Bar()")
    );
    let at = TextSize::from(text.find("$f =").unwrap() as u32);
    let change = ws.rename(id, at, "$obj").unwrap();
    assert!(apply(text, &change, id).contains("$obj = new Foo();\necho $obj->count;"));
    assert!(ws.prepare_rename(id, TextSize::from(text.find("this").unwrap() as u32)).is_err());
}

/// Cases found by the rename consistency check (`examples/rename_check.rs`).
#[test]
fn rename_keeps_code_valid() {
    let rename = |text: &str, at: &str, new: &str| -> Result<String, String> {
        let mut ws = Workspace::new(Config::default());
        let id = ws.set_file("file:///r.php", text.to_string());
        let at = TextSize::from(text.find(at).unwrap() as u32 + 1);
        ws.rename(id, at, new).map(|c| apply(text, &c, id)).map_err(|e| e.0)
    };
    // closures importing the variable
    let text = "<?php
$v = 1;
$f = function () use ($v) { return $v; };
echo $v;
";
    assert_eq!(
        rename(text, "$v = 1", "$w").unwrap(),
        "<?php
$w = 1;
$f = function () use ($w) { return $w; };
echo $w;
"
    );
    assert_eq!(rename(text, "$v; }", "$w").unwrap(), rename(text, "$v = 1", "$w").unwrap());
    // an import alias is renamed in this file only
    let text = "<?php
use Monolog\\Formatter\\JsonFormatter as BaseFormatter;
/** @var BaseFormatter $f */
class F extends BaseFormatter {}
";
    assert_eq!(
        rename(text, "BaseFormatter {}", "Json").unwrap(),
        "<?php
use Monolog\\Formatter\\JsonFormatter as Json;
/** @var Json $f */
class F extends Json {}
"
    );
    // promoted parameters keep their `@param` tags
    let text = "<?php
class E {
    /** @param int $id The id. */
    public function __construct(public $id) {}
}
";
    assert!(rename(text, "$id)", "$key").unwrap().contains("@param int $key The id."));
    // a promoted parameter used as variable in the constructor
    let text = "<?php\nclass L {\n    public function __construct(private string $locale) { $this->d = $locale; }\n    function f() { return $this->locale; }\n}\n";
    assert_eq!(
        rename(text, "$locale; }", "$lang").unwrap(),
        "<?php\nclass L {\n    public function __construct(private string $lang) { $this->d = $lang; }\n    function f() { return $this->lang; }\n}\n"
    );
    // inline `@var` tags and the `@param` of a closure's outer function
    let text = "<?php\n/** @param int $n */\nfunction f($n) {\n    /** @var int $m */\n    $m = $n;\n    return fn () => array_map(function () use ($n) { return $n; }, []);\n}\n";
    assert_eq!(
        rename(text, "$m = ", "$k").unwrap(),
        "<?php\n/** @param int $n */\nfunction f($n) {\n    /** @var int $k */\n    $k = $n;\n    return fn () => array_map(function () use ($n) { return $n; }, []);\n}\n"
    );
    assert!(rename(text, "$n; }", "$z")
        .unwrap()
        .starts_with("<?php\n/** @param int $z */\nfunction f($z) {"));
    // `::class` and magic methods cannot be renamed
    let text = "<?php
class A { function __construct() {} }
$a = A::class;
";
    assert!(rename(text, "lass;", "x").is_err());
    assert!(rename(text, "_construct", "x").is_err());
}

#[test]
fn code_actions_fix_things() {
    let models = "<?php\nnamespace App\\Models;\ninterface Shape { public function area(): float; }\nclass User { private string $name; }\n";
    let main = "<?php\nnamespace App;\n\nuse Unused\\Thing;\n\nclass Circle implements Models\\Shape {\n}\n$u = new User();\n";
    let mut ws = Workspace::new(Config::default());
    ws.set_file("file:///m.php", models.to_string());
    let id = ws.set_file("file:///main.php", main.to_string());
    let titles = |offset: usize| -> Vec<(String, String)> {
        ws.code_actions(id, php_ide::TextRange::empty(TextSize::from(offset as u32)))
            .into_iter()
            .map(|a| (a.title.clone(), apply(main, &a.change, id)))
            .collect()
    };
    let acts = titles(main.find("User()").unwrap());
    let import = acts.iter().find(|(t, _)| t == "Import 'App\\Models\\User'").expect("import action");
    assert!(import.1.contains("use Unused\\Thing;\nuse App\\Models\\User;"), "{}", import.1);
    let acts = titles(main.find("Circle").unwrap());
    let imp =
        acts.iter().find(|(t, _)| t.starts_with("Implement 1 missing method")).expect("implement action");
    assert!(imp.1.contains("    public function area(): float\n    {\n        throw new \\Exception('Not implemented');\n    }\n}"), "{}", imp.1);
    let acts = titles(main.find("Thing").unwrap());
    let rm = acts.iter().find(|(t, _)| t == "Remove unused import").expect("remove import");
    assert!(!rm.1.contains("Unused"), "{}", rm.1);
    let acts = titles(main.find("Circle").unwrap());
    let doc = acts.iter().find(|(t, _)| t == "Add PHPDoc").expect("phpdoc");
    assert!(doc.1.contains("/**\n *\n */\nclass Circle"), "{}", doc.1);

    let id2 = ws.file_id("file:///m.php").unwrap();
    let acts =
        ws.code_actions(id2, php_ide::TextRange::empty(TextSize::from(models.find("$name").unwrap() as u32)));
    let gs = acts.iter().find(|a| a.title == "Generate getter and setter for $name").expect("getter/setter");
    let out = apply(models, &gs.change, id2);
    assert!(
        out.contains("public function getName(): string\n    {\n        return $this->name;\n    }"),
        "{out}"
    );
    assert!(out.contains("public function setName(string $name): static"), "{out}");
}

#[test]
fn hierarchies_and_lenses() {
    let text = "<?php\ninterface I { function run(); }\nclass A implements I { function run() { helper(); } }\nclass B extends A { function run() { parent::run(); } }\nfunction helper() {}\nfunction main() { (new B)->run(); helper(); }\n";
    let mut ws = Workspace::new(Config::default());
    let id = ws.set_file("file:///h.php", text.to_string());
    let at = TextSize::from(text.find("helper() {}").unwrap() as u32);
    let item = ws.prepare_call_hierarchy(id, at).remove(0);
    let callers: Vec<String> = ws.incoming_calls(&item).into_iter().map(|c| c.item.name).collect();
    assert_eq!(callers, vec!["run", "main"]);
    let main_at = TextSize::from(text.find("main()").unwrap() as u32);
    let main_item = ws.prepare_call_hierarchy(id, main_at).remove(0);
    let callees: Vec<String> = ws
        .outgoing_calls(&main_item)
        .into_iter()
        .map(|c| c.item.detail.unwrap_or_default() + "::" + &c.item.name)
        .collect();
    assert_eq!(callees, vec!["B::run", "::helper"]);

    let a_at = TextSize::from(text.find("A implements").unwrap() as u32);
    let a = ws.prepare_type_hierarchy(id, a_at).remove(0);
    assert_eq!(ws.supertypes(&a).into_iter().map(|i| i.name).collect::<Vec<_>>(), vec!["I"]);
    assert_eq!(ws.subtypes(&a).into_iter().map(|i| i.name).collect::<Vec<_>>(), vec!["B"]);

    let lenses = ws.code_lenses(id);
    let titles: Vec<String> =
        lenses.iter().filter_map(|l| ws.resolve_code_lens(id, l)).map(|r| r.title).collect();
    assert!(titles.contains(&"2 implementations".to_string()), "{titles:?}");
    assert!(titles.contains(&"implements I::run".to_string()), "{titles:?}");
    assert!(titles.contains(&"overrides A::run".to_string()), "{titles:?}");
    assert!(titles.contains(&"2 references".to_string()), "{titles:?}");
}

#[test]
fn docblock_completion() {
    let f = fixture("<?php\n/**<|> */\nfunction f(int $a, $b): string {}\n");
    let items = f.ws.completions(f.file, f.offset).items;
    let doc = items.iter().find(|i| i.label == "/** */").expect("docblock item");
    assert_eq!(
        doc.insert_text,
        "/**\n * ${1:}\n *\n * @param ${2:int} $a\n * @param ${3:mixed} $b\n * @return ${4:string}\n */"
    );
}

fn action(text: &str, sel: (&str, &str), title: &str) -> String {
    let mut ws = Workspace::new(Config::default());
    let id = ws.set_file("file:///ref.php", text.to_string());
    let start = text.find(sel.0).unwrap();
    let end = if sel.1.is_empty() { start } else { text.find(sel.1).unwrap() + sel.1.len() };
    let range = php_ide::TextRange::new(TextSize::from(start as u32), TextSize::from(end as u32));
    let acts = ws.code_actions(id, range);
    let a = acts
        .iter()
        .find(|a| a.title == title)
        .unwrap_or_else(|| panic!("no '{title}' in {:?}", acts.iter().map(|a| &a.title).collect::<Vec<_>>()));
    apply(text, &a.change, id)
}

#[test]
fn refactorings() {
    let text = "<?php\nfunction f($user) {\n    echo strtoupper($user->getName());\n}\n";
    assert_eq!(
        action(text, ("$user->getName()", "$user->getName()"), "Extract variable"),
        "<?php\nfunction f($user) {\n    $name = $user->getName();\n    echo strtoupper($name);\n}\n"
    );

    let text = "<?php\nclass A\n{\n    public function run($a)\n    {\n        $b = $a * 2;\n        $c = $b + 1;\n        echo $c;\n    }\n}\n";
    let out = action(text, ("$b = $a", "$b + 1;"), "Extract method");
    assert!(out.contains("        $c = $this->extracted($a);\n        echo $c;"), "{out}");
    assert!(out.contains("    private function extracted($a)\n    {\n        $b = $a * 2;\n        $c = $b + 1;\n        return $c;\n    }"), "{out}");

    let text = "<?php\nclass A\n{\n    function f() { return 'pending'; }\n}\n";
    let out = action(text, ("'pending'", "'pending'"), "Extract constant");
    assert!(
        out.contains("private const PENDING = 'pending';") && out.contains("return self::PENDING;"),
        "{out}"
    );

    let text = "<?php\nfunction f() {\n    $x = 1 + 2;\n    return $x * 3;\n}\n";
    assert_eq!(
        action(text, ("$x =", ""), "Inline variable $x"),
        "<?php\nfunction f() {\n    return (1 + 2) * 3;\n}\n"
    );

    let text = "<?php\nclass P\n{\n    private int $a;\n    protected ?string $b;\n\n    public function __construct(int $a, ?string $b = null)\n    {\n        $this->a = $a;\n        $this->b = $b;\n    }\n}\n";
    let out = action(text, ("__construct", ""), "Promote constructor properties");
    assert_eq!(out, "<?php\nclass P\n{\n\n    public function __construct(private int $a, protected ?string $b = null)\n    {\n    }\n}\n");

    let text = "<?php\nclass Q\n{\n    private string $name;\n    private int $age;\n}\n";
    let out = action(text, ("class Q", ""), "Generate constructor");
    assert!(out.contains("    public function __construct(string $name, int $age)\n    {\n        $this->name = $name;\n        $this->age = $age;\n    }"), "{out}");

    let text = "<?php\nfunction g() { return [1, 2]; }\n";
    assert_eq!(
        action(text, ("g()", ""), "Add return type 'array'"),
        "<?php\nfunction g(): array { return [1, 2]; }\n"
    );
}

#[test]
fn phpunit_lenses() {
    let text = "<?php\nnamespace Tests;\nuse PHPUnit\\Framework\\TestCase;\nclass UserTest extends TestCase\n{\n    public function testName(): void {}\n    /** @test */\n    public function itWorks(): void {}\n    private function helper() {}\n}\n";
    let mut ws = Workspace::new(Config::default());
    ws.set_file(
        "file:///stub.php",
        "<?php\nnamespace PHPUnit\\Framework;\nabstract class TestCase {}\n".into(),
    );
    let id = ws.set_file("file:///UserTest.php", text.to_string());
    let filters: Vec<String> = ws
        .code_lenses(id)
        .into_iter()
        .filter(|l| l.kind == php_ide::CodeLensKind::RunTest)
        .filter_map(|l| l.test_filter)
        .collect();
    assert_eq!(filters, vec!["UserTest", "UserTest::testName", "UserTest::itWorks"]);
}

/// Creates a project directory with files and indexes its PHP files.
fn project(name: &str, files: &[(&str, &str)]) -> (Workspace, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("php-lsp-plugin-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
    }
    let mut ws = Workspace::new(Config::default());
    for (path, kind) in php_ide::collect_files(&dir, &php_ide::ScanConfig::default()) {
        let text = std::fs::read_to_string(&path).unwrap();
        ws.index_file(&format!("file://{}", path.display()), text.into(), kind);
    }
    (ws, dir)
}

fn open_with_cursor(ws: &mut Workspace, dir: &std::path::Path, path: &str, text: &str) -> (FileId, TextSize) {
    let offset = TextSize::from(text.find("<|>").unwrap() as u32);
    let id = ws.set_file(&format!("file://{}", dir.join(path).display()), text.replacen("<|>", "", 1));
    (id, offset)
}

#[test]
fn laravel_plugin() {
    let (mut ws, dir) = project(
        "laravel",
        &[
            ("composer.json", "{}"),
            ("artisan", "#!/usr/bin/env php"),
            (".env", "APP_NAME=Laravel\nDB_HOST=127.0.0.1\n"),
            ("config/app.php", "<?php\nreturn ['name' => env('APP_NAME'), 'providers' => ['x' => 1]];\n"),
            ("resources/views/welcome.blade.php", "<h1>Hi</h1>"),
            ("resources/views/admin/dashboard.blade.php", "<h1>Admin</h1>"),
            ("routes/web.php", "<?php\nRoute::get('/', fn () => view('welcome'))->name('home');\nRoute::get('/a', fn () => 1)->name('admin.index');\n"),
            ("lang/en/messages.php", "<?php\nreturn ['welcome' => 'Welcome!'];\n"),
            (
                "vendor/laravel/framework/src/Model.php",
                "<?php\nnamespace Illuminate\\Database\\Eloquent;\nabstract class Model { public function hasMany($related) {} public function belongsTo($related) {} }\nclass Collection {}\nnamespace Illuminate\\Database\\Eloquent\\Relations;\nabstract class Relation {}\nclass HasMany extends Relation {}\nclass BelongsTo extends Relation {}\n",
            ),
            ("app/Models/Post.php", "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nclass Post extends Model { public function title(): string { return ''; } }\n"),
            (
                "app/Models/User.php",
                "<?php\nnamespace App\\Models;\nuse Illuminate\\Database\\Eloquent\\Model;\nuse Illuminate\\Database\\Eloquent\\Relations\\HasMany;\nclass User extends Model {\n    public function posts(): HasMany { return $this->hasMany(Post::class); }\n}\n",
            ),
            ("app/Services/Billing.php", "<?php\nnamespace App\\Services;\nclass Billing { public function charge(): bool { return true; } }\n"),
        ],
    );
    let labels = |ws: &Workspace, id, offset| -> Vec<String> {
        let mut l: Vec<String> = ws.completions(id, offset).items.into_iter().map(|i| i.label).collect();
        l.sort();
        l
    };
    let (id, off) = open_with_cursor(&mut ws, &dir, "app/x.php", "<?php\nconfig('app.<|>');\n");
    let l = labels(&ws, id, off);
    assert!(l.contains(&"app.name".to_string()) && l.contains(&"app.providers.x".to_string()), "{l:?}");
    let (id, off) = open_with_cursor(&mut ws, &dir, "app/x.php", "<?php\nview('<|>');\n");
    assert_eq!(labels(&ws, id, off), vec!["admin.dashboard", "welcome"]);
    let (id, off) = open_with_cursor(&mut ws, &dir, "app/x.php", "<?php\nroute('<|>');\n");
    assert_eq!(labels(&ws, id, off), vec!["admin.index", "home"]);
    let (id, off) = open_with_cursor(&mut ws, &dir, "app/x.php", "<?php\nenv('DB<|>');\n");
    assert_eq!(labels(&ws, id, off), vec!["DB_HOST"]);
    let (id, off) = open_with_cursor(&mut ws, &dir, "app/x.php", "<?php\n__('mess<|>');\n");
    assert_eq!(labels(&ws, id, off), vec!["messages.welcome"]);
    // goto definition of a view
    let (id, off) = open_with_cursor(&mut ws, &dir, "app/x.php", "<?php\nview('wel<|>come');\n");
    let targets = ws.string_targets(id, off);
    assert!(targets[0].0.ends_with("resources/views/welcome.blade.php"), "{targets:?}");
    // types
    let (id, off) = open_with_cursor(
        &mut ws,
        &dir,
        "app/x.php",
        "<?php\n$b = app(\\App\\Services\\Billing::class);\n$b<|>;\n",
    );
    assert_eq!(ws.type_at(id, off).unwrap(), "App\\Services\\Billing");
    let (id, off) = open_with_cursor(
        &mut ws,
        &dir,
        "app/x.php",
        "<?php\nfunction f(\\App\\Models\\User $u) { $u->posts<|>; }\n",
    );
    assert_eq!(
        ws.type_at(id, off).unwrap(),
        "Illuminate\\Database\\Eloquent\\Collection<int, App\\Models\\Post>"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn symfony_and_wordpress_plugins() {
    let (mut ws, dir) = project(
        "symfony",
        &[
            ("composer.json", "{}"),
            ("symfony.lock", "{}"),
            ("templates/home/index.html.twig", "hi"),
            (
                "src/Controller/HomeController.php",
                "<?php\nnamespace App\\Controller;\nuse Symfony\\Component\\Routing\\Attribute\\Route;\nclass HomeController {\n    #[Route('/', name: 'app_home')]\n    public function index() {}\n}\n",
            ),
            ("vendor/psr/ContainerInterface.php", "<?php\nnamespace Psr\\Container;\ninterface ContainerInterface { public function get(string $id); }\n"),
            ("src/Mailer.php", "<?php\nnamespace App;\nclass Mailer {}\n"),
            ("wp/plugin.php", "<?php\ndo_action('my_plugin_init');\n$v = apply_filters('my_plugin_title', 'x');\n"),
        ],
    );
    let labels = |ws: &Workspace, id, offset| -> Vec<String> {
        ws.completions(id, offset).items.into_iter().map(|i| i.label).collect()
    };
    let (id, off) = open_with_cursor(
        &mut ws,
        &dir,
        "src/Controller/X.php",
        "<?php\nclass X { function a($c) { return $c->redirectToRoute('<|>'); } }\n",
    );
    assert_eq!(labels(&ws, id, off), vec!["app_home"]);
    let (id, off) = open_with_cursor(
        &mut ws,
        &dir,
        "src/Controller/X.php",
        "<?php\nclass X { function a($c) { return $c->render('<|>'); } }\n",
    );
    assert_eq!(labels(&ws, id, off), vec!["home/index.html.twig"]);
    let (id, off) = open_with_cursor(&mut ws, &dir, "src/x.php", "<?php\nfunction f(\\Psr\\Container\\ContainerInterface $c) { $m = $c->get(\\App\\Mailer::class); $m<|>; }\n");
    assert_eq!(ws.type_at(id, off).unwrap(), "App\\Mailer");
    let (id, off) = open_with_cursor(&mut ws, &dir, "wp/other.php", "<?php\nadd_action('<|>', 'cb');\n");
    assert_eq!(labels(&ws, id, off), vec!["my_plugin_init"]);
    let (id, off) = open_with_cursor(&mut ws, &dir, "wp/other.php", "<?php\nadd_filter('my<|>', 'cb');\n");
    assert_eq!(labels(&ws, id, off), vec!["my_plugin_title"]);
    let (id, off) =
        open_with_cursor(&mut ws, &dir, "wp/other.php", "<?php\nadd_filter('my_plugin<|>_title', 'cb');\n");
    let targets = ws.string_targets(id, off);
    assert!(targets[0].0.ends_with("wp/plugin.php") && targets[0].1 == 2, "{targets:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn generics_through_inheritance() {
    let lib = r#"<?php
/**
 * @template TKey
 * @template TValue
 * @implements IteratorAggregate<TKey, TValue>
 */
class Collection implements IteratorAggregate {
    /** @return TValue|null */
    public function first() {}
    /** @return static<TKey, TValue> */
    public function filter() {}
    /** @return static<int, static<int, TValue>> */
    public function chunk() {}
    /** @var array<TKey, TValue> */
    public array $items = [];
    public function getIterator(): Traversable {}
}
class User { public function name(): string {} }
/** @extends Collection<int, User> */
class UserList extends Collection {}
/**
 * @template T
 */
class Repo {
    /** @return Collection<int, T> */
    public function all() {}
}
/** @extends Repo<User> */
class UserRepo extends Repo {}
"#;
    let check = |code: &str, expected: &str| {
        let text = format!("//- lib.php\n{lib}//- main.php\n<?php\n{code}\n");
        assert_eq!(type_at(&text), expected, "{code}");
    };
    check("function f(UserList $l) { $u = $l->first(); $u<|>; }", "?User");
    check("function f(UserList $l) { foreach ($l as $u) { $u<|>; } }", "User");
    check("function f(UserRepo $r) { $x = $r->all()->first(); $x<|>; }", "?User");
    check("function f(UserRepo $r) { foreach ($r->all() as $u) { $u<|>; } }", "User");
    check(
        "/** @param Collection<string, User> $c */ function f($c) { $x = $c->items; $x<|>; }",
        "array<string, User>",
    );
    // `static<...>` keeps its arguments
    check(
        "/** @param Collection<string, User> $l */ function f($l) { foreach ($l->chunk() as $c) { $c<|>; } }",
        "Collection<int, User>",
    );
}

#[test]
fn phpdoc_type_references() {
    let text = "<?php\nnamespace App;\nclass User {}\n/**\n * @param User $u\n * @return array<int, User>\n */\nfunction f($u) { return [$u]; }\n/** @var Nope $x */\n$x = 1;\n";
    let mut ws = Workspace::new(Config::default());
    let id = ws.set_file("file:///d.php", text.to_string());
    // hover / definition from the doc comment
    let at = TextSize::from(text.find("User $u").unwrap() as u32 + 1);
    assert_eq!(
        ws.definition_at(id, at),
        Some(php_ide::Definition::Class(php_ide::hir::Fqn::new("App\\User")))
    );
    // references include doc comments; rename changes them
    let at = TextSize::from(text.find("class User").unwrap() as u32 + 7);
    assert_eq!(ws.references(id, at, true).len(), 3);
    let change = ws.rename(id, at, "Member").unwrap();
    let out = apply(text, &change, id);
    assert!(
        out.contains("@param Member $u")
            && out.contains("array<int, Member>")
            && out.contains("class Member"),
        "{out}"
    );
    // undefined types in docs
    let d: Vec<String> = ws.diagnostics(id).into_iter().map(|d| d.message).collect();
    assert_eq!(d, vec!["PHPDoc undefined type 'App\\Nope'."]);
}

#[test]
fn document_links_and_file_rename() {
    let (mut ws, dir) = project(
        "links",
        &[
            ("composer.json", r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#),
            ("lib/helpers.php", "<?php\nfunction helper() {}\n"),
            ("src/Models/User.php", "<?php\nnamespace App\\Models;\n\nclass User {}\n"),
            ("src/Http/Controller.php", "<?php\nnamespace App\\Http;\n\nuse App\\Models\\User;\n\nclass Controller { public function f(User $u): \\App\\Models\\User { return $u; } }\n"),
        ],
    );
    let main = "<?php\nrequire __DIR__ . '/lib/helpers.php';\ninclude 'missing.php';\n// see https://php.net/manual.\n";
    let id = ws.set_file(&format!("file://{}", dir.join("index.php").display()), main.into());
    let links = ws.document_links(id);
    let targets: Vec<_> = links.iter().map(|l| l.target.clone()).collect();
    assert_eq!(
        targets,
        vec![
            format!("file://{}", dir.join("lib/helpers.php").display()),
            "https://php.net/manual".to_string()
        ]
    );

    // move src/Models/User.php -> src/Entities/Account.php
    let user_key = format!("file://{}", dir.join("src/Models/User.php").display());
    let user = ws.file_id(&user_key).unwrap();
    let new_key = format!("file://{}", dir.join("src/Entities/Account.php").display());
    let change = ws.file_rename_edits(user, &new_key);
    let user_text = ws.file(user).unwrap().text.to_string();
    assert_eq!(apply(&user_text, &change, user), "<?php\nnamespace App\\Entities;\n\nclass Account {}\n");
    let ctrl = ws.file_id(&format!("file://{}", dir.join("src/Http/Controller.php").display())).unwrap();
    let ctrl_text = ws.file(ctrl).unwrap().text.to_string();
    assert_eq!(
        apply(&ctrl_text, &change, ctrl),
        "<?php\nnamespace App\\Http;\n\nuse App\\Entities\\Account;\n\nclass Controller { public function f(Account $u): \\App\\Entities\\Account { return $u; } }\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn html_in_templates() {
    let labels = |text: &str| completion_labels(text);
    assert!(labels("<div><sp<|>").contains(&"span".to_string()));
    let attrs = labels("<?php $x = 1; ?>\n<a class=\"<?= $x ?>\" hr<|>");
    assert!(attrs.contains(&"href".to_string()) && attrs.contains(&"class".to_string()), "{attrs:?}");
    assert_eq!(labels("<ul><li><?php echo 1; ?></li></<|>"), vec!["ul".to_string()]);
    assert_eq!(labels("<script>if (a < b) {}</script><p></<|>"), vec!["p".to_string()]);
    assert!(labels("<a href=\"x<|>\">").is_empty());
    assert!(labels("<p></p><<|>/div>").is_empty());

    let Fixture { ws, file, offset } =
        fixture("<div class=\"a\">\n  <br><di<|>v>x</div>\n<?php if ($a): ?></div><?php endif; ?>\n</div>\n");
    let text = ws.file(file).unwrap().text.to_string();
    let ranges = ws.linked_editing_ranges(file, offset).unwrap();
    let names: Vec<_> = ranges
        .iter()
        .map(|r| (&text[usize::from(r.start())..usize::from(r.end())], usize::from(r.start())))
        .collect();
    assert_eq!(names.len(), 2);
    assert_eq!(names[0].0, "div");
    assert_eq!(names[1], ("div", text.find("x</div>").unwrap() + 3));
}

#[test]
fn blade_templates() {
    let (mut ws, dir) = project(
        "blade",
        &[
            ("composer.json", "{}"),
            ("artisan", "#!/usr/bin/env php"),
            ("app/Models/User.php", "<?php\nnamespace App\\Models;\n/** A user. */\nclass User {\n    /** @var Post[] */\n    public array $posts = [];\n    public function displayName(): string { return ''; }\n}\nclass Post { public string $title = ''; }\n"),
            (
                "app/Http/UserController.php",
                "<?php\nnamespace App\\Http;\nuse App\\Models\\User;\nclass UserController {\n    public function show(User $user) {\n        $count = 3;\n        return view('users.show', ['user' => $user])->with('count', $count);\n    }\n    public function list() { $users = [new User]; return view('users.list', compact('users')); }\n}\n",
            ),
            ("resources/views/layouts/app.blade.php", "<html>@yield('content')</html>"),
            ("resources/views/users/list.blade.php", "@foreach($users as $u) {{ $u->displayName() }} @endforeach"),
        ],
    );
    let path = "resources/views/users/show.blade.php";
    let template = "@extends('layouts.app')\n@section('content')\n<div class=\"user\">\n  <h1>{{ $user->displayName() }}</h1>\n  @foreach($user->posts as $post)\n    <p>{{ $post->title }} {{ $loop->index }}</p>\n  @endforeach\n  {{ $count }} {{ $user->missing() }}\n  @if($errors->any()) x @endif\n</div>\n@endsection\n";
    let key = format!("file://{}", dir.join(path).display());
    let id = ws.set_file(&key, template.into());
    let at = |needle: &str, delta: usize| TextSize::from((template.find(needle).unwrap() + delta) as u32);

    // types flow from the controller into the template
    assert_eq!(ws.type_at(id, at("$user->displayName", 1)).unwrap(), "App\\Models\\User");
    assert_eq!(ws.type_at(id, at("$post->title", 1)).unwrap(), "App\\Models\\Post");
    assert_eq!(ws.type_at(id, at("$count }}", 1)).unwrap(), "int");
    assert_eq!(ws.type_at(id, at("$errors", 1)).unwrap(), "Illuminate\\Support\\ViewErrorBag");
    assert!(ws.hover(id, at("displayName", 2)).unwrap().markdown.contains("displayName(): string"));
    let defs = ws.goto_definition(id, at("displayName", 2));
    assert!(ws.file(defs[0].file).unwrap().path.ends_with("app/Models/User.php"));

    // member completion inside an echo
    let labels = |ws: &Workspace, offset: TextSize| -> Vec<String> {
        ws.completions(id, offset).items.into_iter().map(|i| i.label).collect()
    };
    assert!(labels(&ws, at("$user->displayName", 7)).contains(&"displayName".to_string()));
    // directives and HTML outside of PHP code
    let (id2, off) =
        open_with_cursor(&mut ws, &dir, "resources/views/x.blade.php", "<div>\n  @fore<|>\n</div>");
    let l: Vec<String> = ws.completions(id2, off).items.into_iter().map(|i| i.label).collect();
    assert!(l.contains(&"@foreach".to_string()) && l.contains(&"@forelse".to_string()), "{l:?}");
    let (id2, off) =
        open_with_cursor(&mut ws, &dir, "resources/views/x.blade.php", "<div>\n  {{ $a }} <sp<|>\n</div>");
    let l: Vec<String> = ws.completions(id2, off).items.into_iter().map(|i| i.label).collect();
    assert!(l.contains(&"span".to_string()), "{l:?}");
    // view names in directives
    let l = labels(&ws, at("layouts.app", 1));
    assert!(l.contains(&"layouts.app".to_string()) && l.contains(&"users.list".to_string()), "{l:?}");
    assert!(ws.string_targets(id, at("layouts.app", 2))[0].0.ends_with("layouts/app.blade.php"));

    // diagnostics: only the real problem
    let diags: Vec<String> = ws.diagnostics(id).into_iter().map(|d| d.message).collect();
    assert_eq!(diags, vec!["Undefined method 'missing'.".to_string()]);
    // compact()
    let list = ws
        .file_id(&format!("file://{}", dir.join("resources/views/users/list.blade.php").display()))
        .unwrap();
    let text = ws.file(list).unwrap().text.to_string();
    assert_eq!(
        ws.type_at(list, TextSize::from(text.find("$u->").unwrap() as u32 + 1)).unwrap(),
        "App\\Models\\User"
    );
    let php_only = php_ide::FormatOptions { format_html: false, ..Default::default() };
    assert!(ws.format(id, None, &php_only).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn blade_structure() {
    let mut ws = Workspace::new(Config::default());
    ws.load_stubs();
    let text = "@extends('layouts.app')\n@section('title', 'Users')\n@section('content')\n  @foreach($users as $u)\n    {{ $loop->index }} {{ $u }}\n  @endforeach\n  @php\n    $x = 1;\n  @endphp\n  @push('scripts')\n    <script></script>\n  @endpush\n@endsection\n";
    let id = ws.set_file("file:///p/resources/views/users.blade.php", text.into());
    let at = |needle: &str, delta: usize| TextSize::from((text.find(needle).unwrap() + delta) as u32);
    // $loop
    assert_eq!(ws.type_at(id, at("$loop", 1)).unwrap(), "PhpLsp\\Blade\\Loop");
    let l: Vec<String> = ws.completions(id, at("$loop->", 7)).items.into_iter().map(|i| i.label).collect();
    assert!(l.contains(&"index".to_string()) && l.contains(&"first".to_string()), "{l:?}");
    // folding: section, foreach, php, push
    let folds: Vec<&str> =
        ws.folding_ranges(id).iter().map(|f| text[f.range].split(['(', '\n']).next().unwrap()).collect();
    assert_eq!(folds, vec!["@section", "@foreach", "@php", "@push"]);
    // outline
    let syms = ws.document_symbols(id);
    let names: Vec<(&str, Vec<&str>)> = syms
        .iter()
        .map(|s| (s.name.as_str(), s.children.iter().map(|c| c.name.as_str()).collect()))
        .collect();
    assert_eq!(names, vec![("title", vec![]), ("content", vec!["scripts"])]);
    // directives are highlighted
    let kw: Vec<&str> = ws
        .semantic_tokens(id, None)
        .into_iter()
        .filter(|t| t.token_type == php_ide::SemanticTokenType::Keyword)
        .map(|t| &text[t.range])
        .collect();
    assert_eq!(&kw[..5], &["@extends", "@section", "@section", "@foreach", "{{"]);
    let tokens = ws.semantic_tokens(id, None);
    assert!(tokens.windows(2).all(|w| w[0].range.start() <= w[1].range.start()));
}

#[test]
fn language_constraints_and_suspect_code() {
    let src = r#"<?php
interface I { private function a(); function b() {} }
abstract class A { abstract private function c(); abstract function d() {} function e(); }
class B { abstract function f(); public public function g() {} readonly $r; public readonly int $s = 1; public static readonly int $t;
  function __construct(private int $p) {}
  function __construct2(private int $q) {}
  public function __destruct(): void {}
  private function hidden(): int { return 1; }
  protected int $prot = 1;
  /** @param int $nope */
  function h(int $x = 1, int $y, int ...$z) { return [$x, $y, $z]; }
  static function st() { return $this; }
  function k() { return parent::k(); }
  function m() { return [1 => 'a', '1' => 'b', 'x' => 1, 'x' => 2]; }
  function m() {}
}
function fn1() { break; }
function fn2(int $a, int $b) { return $a + $b; }
fn2(a: 1, 2);
fn2(a: 1, a: 2);
fn2(c: 1, b: 2);
function fn3(B $b) { $b->hidden(); return $b->prot; }
function fn4($v) { if ($v = 1) { return 'a' + 1; } elseif ($v) { return 1; } elseif ($v) { return 2; } return match ($v) { 1 => 'a', 1 => 'b', default => 'c' }; }
function fn5(): void {}
function fn6() { $x = fn5(); return $x; }
"#;
    let d = diags(src);
    let codes: Vec<&str> = d.iter().map(|x| &x[..5]).collect();
    for c in [
        "P1019", "P1021", "P1017", "P1020", "P1029", "P1015", "P1046", "P1047", "P1057", "P1041", "P1101",
        "P1130", "P1030", "P1035", "P1118", "P1004", "P1104", "P1042", "P1043", "P1044", "P1080", "P1122",
        "P1121", "P1123", "P1125",
    ] {
        assert!(codes.contains(&c), "missing {c} in {d:#?}");
    }
    let access: Vec<&String> = d.iter().filter(|x| x.starts_with("P1080")).collect();
    assert_eq!(
        access,
        vec![
            "P1080 Cannot access private method B::hidden from global scope.",
            "P1080 Cannot access protected property B::$prot from global scope."
        ]
    );
}

#[test]
fn argument_type_errors() {
    let src = r#"<?php
interface Shape {}
class Circle implements Shape { function __toString(): string { return ''; } }
class Other {}
function area(Shape $s): float { return 1.0; }
function name(string $n): string { return $n; }
function nums(array $a, ?int $i = null, int|string $u = 1, iterable $it = [], callable $c = null) {}
area(new Circle());
area(new Other());
area(null);
name(new Circle());
name(42);
name([]);
nums([1], null, 'x', [], fn () => 1);
nums('x');
nums([], new Other());
"#;
    let d: Vec<String> = diags(src).into_iter().filter(|d| d.starts_with("P1006")).collect();
    assert_eq!(
        d,
        vec![
            "P1006 Expected type 'Shape'. Found 'Other'.",
            "P1006 Expected type 'Shape'. Found 'null'.",
            "P1006 Expected type 'string'. Found 'array'.",
            "P1006 Expected type 'array'. Found 'string'.",
            "P1006 Expected type '?int'. Found 'Other'.",
        ]
    );
}

#[test]
fn documented_type_compatibility() {
    let src = r#"<?php
class Base {} class Sub extends Base {}
/**
 * @param string $a wrong
 * @param int[] $b fine
 * @param Sub $c fine
 * @param int|null $d fine
 * @param callable $e fine
 * @return Base wrong
 */
function dc(int $a, array $b, Base $c, ?int $d, \Closure $e): string { return ''; }
"#;
    let d: Vec<String> = diags(src).into_iter().filter(|d| d.starts_with("P1131")).collect();
    assert_eq!(d, vec!["P1131 Documented type is not compatible with the declared type."; 2]);
}

#[test]
fn catalog_round_two() {
    let src = r#"<?php
class Base {
    public function a(int $x): ?Base { return null; }
    public function b(string $s, int $o = 0): string { return $s; }
    final public function c(): void {}
    public function d(Base $b): static { return $this; }
}
final class Locked {}
class Child extends Base {
    public function a(int $x, int $y): Base { return $this; }
    public function b(string $s, int $o = 0, int ...$rest): string { return $s; }
    public function c(): void {}
    public function d(object $b): static { return $this; }
}
class Fine extends Base {
    public function a(int|string $x, int $y = 0): Child { return new Child(); }
}
class Bad extends Locked {}
trait T1 { function go(): int { return 1; } }
trait T2 { function go(): int { return 2; } }
class Collide { use T1, T2; }
class Resolved { use T1, T2 { T1::go insteadof T2; } }
class Plain {}
#[Plain]
function attr_target() {}
#[Attribute]
class Once {}
#[Once, Once]
function twice() {}
enum Backed: string { case A = 'a'; case B; case C = 'a'; public function __get($n) {} }
enum Pure { case X = 1; }
"#;
    let d = diags(src);
    let codes: Vec<&str> = d.iter().map(|x| &x[..5]).collect();
    for (code, n) in [
        ("P1038", 1),
        ("P1039", 1),
        ("P1094", 1),
        ("P1093", 1),
        ("P1086", 1),
        ("P1088", 1),
        ("P1050", 1),
        ("P1052", 1),
        // Intelephense reports every case sharing the value
        ("P1103", 2),
        ("P1096", 1),
    ] {
        assert_eq!(codes.iter().filter(|c| **c == code).count(), n, "{code} in {d:#?}");
    }
    assert!(
        d.contains(&"P1038 Method 'Child::a()' is not compatible with method 'Base::a()'.".to_string()),
        "{d:#?}"
    );
    assert!(d.contains(&"P1094 Class Bad cannot extend final class Locked.".to_string()), "{d:#?}");
    assert!(d.contains(&"P1052 Case X of non-backed enum Pure must not have a value.".to_string()), "{d:#?}");
}

/// False positives found in the third comparison run.
#[test]
fn comparison_regressions() {
    let src = r#"<?php
declare(strict_types=1);
class Node {}
class Expression {}
/** @template TMin */
function f1(array $conds, ?array $data, $v): string {
    for ($i = 0; $i + 1 < count($conds); $i++) {}
    echo $conds[$i];
    if (!(is_array($data) && is_string($id = $data['id'] ?? null))) { return ''; }
    echo $id;
    $m = array_map(floatval(...), [1]);
    $t = $data + [1];
    if (is_array($v)) { $q = http_build_query($v); }
    if ($v instanceof Expression) { $v = 'x'; echo strlen($v); }
    echo str_repeat(Node::class, 1);
    set_error_handler(function ($errno, $errstr) { return $errstr !== ''; });
    foreach ($conds as $k => $unusedValue) { echo $k; }
    return implode(',', [$m, $t, $q ?? '']);
}
function f2(int $a, string $optional = ''): int { return $a; }
class Ns {} class Arr { public ?Ns $value = null; }
function f4(array $stmts, Arr $item, bool $c): void {
    $last = null;
    foreach ($stmts as $s) { if ($last !== null) { f5($last); } $last = $s instanceof Ns ? $s : new Ns(); }
    if ($item->value instanceof Ns) { f5($item->value); }
    if ($c) { $shared = new Ns(); }
    if (empty($shared)) { $shared = new Ns(); }
    f5($shared);
}
function f5(Ns $n): void { echo get_class($n); }
class Named { function getName(): string { return ''; } }
function f8(?object $t, iterable $it, ?int $maybe): string {
    if (!$t instanceof Named || rand()) { return ''; }
    $n = $t->getName();
    if ($maybe === null) { throw new \Exception(); }
    if (rand()) { $o = 1; } elseif (false === $o = f9()) { $o = 2; }
    return $n . iterator_count($it) . count(iterator_to_array($it)) . $o;
}
function f9(): int|false { return 1; }
function f6(string|array $contents): int { if (is_string($contents)) { $contents = 'x'; return strlen($contents); } else { return count($contents); } }
function f3(): int { while (true) { switch (rand()) { case 1: break; default: return 1; } } }
function f7(): int { for (;;) { for (;;) { switch (rand()) { case 1: break 2; } } if (rand()) { return 1; } } }
"#;
    let d = diags(src);
    assert_eq!(d, vec!["P1132 Parameter $v has no type information available."]);
}

/// Differences found by comparing with Intelephense (editors/compare).
#[test]
fn intelephense_parity() {
    let classes = "class Token { public string $text = ''; }\nabstract class Stmt {}\nclass ClassMethod extends Stmt { public string $name = ''; }\ninterface Node {}\n";
    // element of a typed array with an expression index
    assert_eq!(
        type_at(&format!("<?php\n{classes}/** @param Token[] $tokens */\nfunction f(array $tokens, int $i) {{ $tokens[$i + 1]<|>; }}\n")),
        "Token"
    );
    // narrowing on the right of `&&` in a foreach over a typed property
    let d = diags(&format!("<?php\n{classes}class C {{\n  /** @var Stmt[] */\n  public array $stmts = [];\n  function f() {{\n    foreach ($this->stmts as $stmt) {{\n      if ($stmt instanceof ClassMethod && $stmt->name === 'x') {{ return $stmt; }}\n    }}\n  }}\n}}\n"));
    assert!(d.is_empty(), "{d:?}");
    // properties on interfaces are not reported (implementations may declare them)
    let d = diags(&format!("<?php\n{classes}function g(Node $n) {{ return $n->foo; }}\n"));
    assert!(d.is_empty(), "{d:?}");
    // parameter types documented on the overridden method
    assert_eq!(
        type_at(&format!("<?php\n{classes}abstract class E {{ /** @param Token[] $tokens */ abstract function emulate(array $tokens); }}\nclass F extends E {{ function emulate(array $tokens) {{ $tokens[0]<|>; }} }}\n")),
        "Token"
    );
    // `$name` in `@param` refers to the parameter
    let f = fixture("<?php\nclass Token {}\nclass K {\n  /** @param Token $to<|>ken the token */\n  function f($token) {}\n}\n");
    let defs = f.ws.goto_definition(f.file, f.offset);
    assert_eq!(defs.len(), 1);
    assert!(f.ws.hover(f.file, f.offset).unwrap().markdown.contains("Token"));
    // no type information (P1132): not for typed, documented, defaulted or inherited
    let d = diags(&format!("<?php\n{classes}abstract class P {{ abstract function m(int $a); /** @var int */ public $p; }}\nclass Q extends P {{\n  public $p;\n  public $u;\n  public $w = 1;\n  /** @param int $b */\n  function m($a, $b = 1, $c = null) {{}}\n  function n($d) {{}}\n}}\n"));
    assert_eq!(
        d,
        vec![
            "P1132 Property $u has no type information available.",
            "P1003 Symbol '$d' is declared but not used.",
            "P1132 Parameter $d has no type information available."
        ]
    );
    // stub functions documented `@since 8.5` do not exist for PHP 8.4
    let mut ws = Workspace::new(Config { php_version: php_ide::PhpVersion::new(8, 4), ..Config::default() });
    ws.load_stubs();
    let id = ws.set_file("file:///v.php", "<?php\nget_exception_handler();\nstrlen('');\n".into());
    let d: Vec<String> = ws.diagnostics(id).into_iter().map(|d| d.message).collect();
    assert_eq!(d, vec!["Undefined function 'get_exception_handler'."]);
    // trait method aliases
    let d = diags("<?php\ntrait R { function many(): int { return 1; } }\nclass S { use R { many as private manyAlias; } function f() { return $this->manyAlias(); } }\n");
    assert!(d.is_empty(), "{d:?}");
    // guarded by method_exists / property_exists
    let d = diags("<?php\nclass N {}\nfunction h(N $n) { if (method_exists($n, 'via')) { $n->via(); } return property_exists($n, 'p') ? $n->p : null; }\n");
    assert!(d.is_empty(), "{d:?}");
    // unused parameters (not of overriding methods, by-ref or promoted ones)
    let d = diags("<?php\nfunction k(int $unused) { return array_map(function (int $a, int $b) { return $a; }, [1], [2]); }\nabstract class B0 { function o(int $x) { return $x; } }\nclass B1 extends B0 { function __construct(private int $p, int &$r) {} function o(int $x) { return 1; } }\n");
    assert_eq!(
        d,
        vec![
            "P1003 Symbol '$unused' is declared but not used.",
            "P1003 Symbol '$b' is declared but not used."
        ]
    );
    // imported functions that do not exist
    let d = diags("<?php\nnamespace A;\nuse function Lib\\select;\nselect();\n");
    assert_eq!(d, vec!["P1010 Undefined function 'Lib\\select'."]);
    // unreachable code
    let d = diags("<?php\nfunction u(int $x) { if ($x) { return 1; echo 2; echo 3; } switch ($x) { case 1: break; echo 4; } throw new \\Exception(); function later() {} }\n");
    assert_eq!(d, vec!["P1108 Unreachable code detected."; 2]);
    // optional dependencies, `??`, `$this` in (re-bindable) closures
    let d = diags("<?php\nclass M { function f(M $m) {\n  if (class_exists(\\Opt\\Dep::class)) { \\Opt\\Dep::run(); }\n  $g = $m->group ?? null;\n  $c = fn () => $this->enabled;\n  return [$g, $c];\n} }\n");
    assert!(d.is_empty(), "{d:?}");
    // deprecated imports
    let d = diags("<?php\nnamespace Old { /** @deprecated use New */ class Cache {} }\nnamespace App { use Old\\Cache; use function strlen; }\n");
    assert_eq!(
        d,
        vec![
            "P1007 'Old\\Cache' is deprecated. use New",
            "P1003 Symbol 'Old\\Cache' is declared but not used.",
            "P1003 Symbol 'strlen' is declared but not used."
        ]
    );
    // negated instanceof on the left of ||
    assert_eq!(
        type_at(&format!(
            "<?php\n{classes}function o(Stmt $t) {{ return !$t instanceof ClassMethod || $t<|>; }}\n"
        )),
        "ClassMethod"
    );
    // members of anonymous classes
    let d = diags(&format!("<?php\n{classes}function a() {{ $p = new class extends Stmt {{ public int $actual = 1; function run(): int {{ return $this->actual; }} }}; return $p->actual + $p->run(); }}\n"));
    assert!(d.is_empty(), "{d:?}");
    assert_eq!(type_at(&format!("<?php\n{classes}function a() {{ $p = new class extends Stmt {{ public int $actual = 1; }}; $p->actual<|>; }}\n")), "int");
    // P1124 / P1128 / P1075
    let d = diags("<?php\nfunction lp(array $items, int $item) { foreach ($items as $item) { echo $item; } }\nclass OB { function a(): int { return 1; } }\nclass OC extends OB {\n  #[\\Override] function a(): int { return 2; }\n  #[\\Override] function b(): int { if (rand()) { return 1; } }\n  function c(): int { if (rand()) { return 1; } else { throw new \\Exception(); } }\n  function d(int $x): string { switch ($x) { case 1: return 'a'; default: return 'b'; } }\n  function e(): iterable { yield 1; }\n  function f(): int { while (true) { } }\n}\n");
    assert_eq!(
        d,
        vec![
            "P1124 Loop variable $item is also declared as a parameter.",
            "P1128 OC::b() has #[\\Override] attribute, but no matching parent method exists.",
            "P1075 Not all paths return a value."
        ]
    );
    // P1116 possibly undefined variables
    let d = diags("<?php\nfunction pu(int $a) {\n  if ($a) { $x = 1; }\n  echo $x;\n  if ($a) { $y = 1; } else { $y = 2; }\n  echo $y;\n  foreach ([1] as $v) { $z = $v; }\n  echo $z;\n  $a && ($w = 1);\n  echo $w;\n  if (preg_match('/x/', 'y', $m)) {}\n  echo $m[0];\n  try { $t = 1; } catch (\\Exception) { return; }\n  echo $t;\n}\n");
    assert_eq!(
        d,
        vec![
            "P1116 Possible undefined variable '$x'.",
            "P1116 Possible undefined variable '$z'.",
            "P1116 Possible undefined variable '$w'."
        ]
    );
    // && in conditions, by-ref outputs, arrow fn params, `$this` in anon class arguments
    let d = diags("<?php\nclass Q2 { private int $store = 1;\n  function a(?int &$code, $h): int|false {\n    $code = 0;\n    if (rand() && $p = $this->b()) { return $p; }\n    $f = fn (string $unused, int $u) => $u;\n    $o = new class($this->store) { function __construct(public int $s) {} };\n    return $f('x', 1) + $o->s + $h;\n  }\n  function b(): int { return 1; }\n}\n");
    assert_eq!(
        d,
        vec![
            "P1132 Parameter $h has no type information available.",
            "P1003 Symbol '$unused' is declared but not used."
        ]
    );
    let d = diags("<?php\nfunction w(?object $j): int { if (!$j || is_null($u = $j->id ?? null)) { return 0; } return $u; }\n");
    assert!(d.is_empty(), "{d:?}");
    let d = diags("<?php\nfunction r1($f, ?string $n) { if ($f) { $c = 1; } else { return 0; } if ($n !== null) { $u = 1; } if (isset($i)) { echo $i; } if ($n !== null) { echo $u; } return $c; }\n");
    assert_eq!(d, vec!["P1132 Parameter $f has no type information available."]);
    let d = diags("<?php\nclass PB { const X = 1; }\nclass PF { const X = PB::X; const Y = PB::X;\n  /**\n   * @param string|array<int, covariant $this> $r\n   */\n  function f($r, bool $c, object $m) { if ($c) { $k = 1; } if ($m instanceof PB) { $g = 1; } $o = $c ? $k : 0; return [$r, $o, $m instanceof PB && $g, rand() && ($i = 1) && $i]; }\n}\n");
    assert!(d.is_empty(), "{d:?}");
    let d = diags("<?php\nfunction sd(bool $s) { if (!$s) { echo 1; } else { $d = 2; } if (!$s) { return 0; } else { return $d; } }\n");
    assert!(d.is_empty(), "{d:?}");
    // narrowing after a guard that leaves the block
    assert_eq!(type_at(&format!("<?php\n{classes}function gd(?Stmt $s) {{ if (!$s instanceof ClassMethod) {{ return; }} $s<|>; }}\n")), "ClassMethod");
    assert_eq!(type_at(&format!("<?php\n{classes}function gn(?Token $s) {{ if ($s === null) {{ throw new \\Exception(); }} $s<|>; }}\n")), "Token");
    // attribute classes
    let d = diags(
        "<?php\nnamespace A;\nuse Sym\\AsCommand;\n#[AsCommand('x'), \\Attribute, \\Override]\nclass C {}\n",
    );
    assert_eq!(d, vec!["P1009 Undefined type 'Sym\\AsCommand'."]);
    // `Foo::CONST_*` in PHPDoc is a constant pattern, not a class
    let d = diags(&format!(
        "<?php\n{classes}class U {{ const TYPE_A = 1; /** @var U::TYPE_* */ public int $t = 1; }}\n"
    ));
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn blade_php_blocks_are_formatted() {
    let mut ws = Workspace::new(Config::default());
    let text = "<div>\n    @php\n$items=[1,2];\n        if($x){\n$y=1;\n}\n    @endphp\n    {{  $x  }}\n    @php $a=1; @endphp\n    @php\n$s = 'a\nb';\n    @endphp\n</div>\n";
    let id = ws.set_file("file:///v.blade.php", text.to_string());
    // without HTML formatting only `@php` blocks change
    let php_only = php_ide::FormatOptions { format_html: false, ..Default::default() };
    let edits = ws.format(id, None, &php_only).unwrap();
    let mut out = text.to_string();
    for e in edits.iter().rev() {
        out.replace_range(usize::from(e.range.start())..usize::from(e.range.end()), &e.new_text);
    }
    assert_eq!(
        out,
        "<div>\n    @php\n        $items = [1, 2];\n        if ($x) {\n            $y = 1;\n        }\n    @endphp\n    {{  $x  }}\n    @php $a=1; @endphp\n    @php\n$s = 'a\nb';\n    @endphp\n</div>\n"
    );
    // idempotent
    ws.set_file("file:///v.blade.php", out.clone());
    assert!(ws.format(id, None, &php_only).unwrap().is_empty());
    // range formatting only touches blocks inside the range
    ws.set_file("file:///v.blade.php", text.to_string());
    let r = php_ide::TextRange::new(TextSize::from(0), TextSize::from(3));
    assert!(ws.format(id, Some(r), &php_only).unwrap().is_empty());
}

/// Blade views are formatted as HTML like Intelephense does (js-beautify).
#[test]
fn blade_views_are_formatted_as_html() {
    let mut ws = Workspace::new(Config::default());
    let text = "<div>\n<ul>\n@foreach ($items as $i)\n<li>{{ $i }}</li>\n@endforeach\n</ul>\n</div>\n";
    let id = ws.set_file("file:///v.blade.php", text.to_string());
    let edits = ws.format(id, None, &Default::default()).unwrap();
    let mut out = text.to_string();
    for e in edits.iter().rev() {
        out.replace_range(usize::from(e.range.start())..usize::from(e.range.end()), &e.new_text);
    }
    assert_eq!(out, "<div>\n    <ul>\n        @foreach ($items as $i)\n        <li>{{ $i }}</li>\n        @endforeach\n    </ul>\n</div>");
}

#[test]
fn instanceof_unrelated_interface_intersects() {
    let d = diags(
        "<?php\ninterface A { public function a(); }\ninterface B { public function b(); }\nfunction g(A $a) {}\nfunction f(A $x) {\n    if (!$x instanceof B) {\n        return;\n    }\n    g($x);\n    $x->a();\n    $x->b();\n}\n",
    );
    assert!(d.iter().all(|m| !m.starts_with("P1006") && !m.starts_with("P1013")), "{d:#?}");
    assert_eq!(
        type_at(
            "<?php\ninterface A {}\ninterface B {}\nfunction f(A $x) { if ($x instanceof B) { $x<|>; } }"
        ),
        "A&B"
    );
}

#[test]
fn switch_default_and_infinite_loop_define() {
    let d = diags(
        "<?php\nfunction f($m) {\n    switch ($m) {\n        case 'a':\n            $q = 1;\n            break;\n        case 'b':\n        default:\n            $q = 2;\n    }\n    return $q;\n}\nfunction g($f) {\n    while (true) {\n        $r = fgets($f);\n        if (!$r) {\n            break;\n        }\n    }\n    return $r;\n}\nfunction h($m) {\n    switch ($m) {\n        case 'a':\n            $s = 1;\n            break;\n        case 'b':\n            break;\n        default:\n            $s = 2;\n    }\n    return $s;\n}\n",
    );
    let p: Vec<&String> = d.iter().filter(|m| m.starts_with("P1116")).collect();
    assert_eq!(p, vec!["P1116 Possible undefined variable '$s'."], "{d:#?}");
}

/// Catalog checks compared with the real Intelephense (one snippet per case,
/// expected codes and messages taken from its output for that PHP version).
#[test]
fn catalog_round_three() {
    const CODES: &[&str] = &[
        "P1022", "P1023", "P1025", "P1031", "P1032", "P1033", "P1034", "P1045", "P1048", "P1049", "P1051",
        "P1055", "P1058", "P1059", "P1060", "P1061", "P1062", "P1063", "P1064", "P1065", "P1067", "P1068",
        "P1069", "P1071", "P1074", "P1076", "P1082", "P1083", "P1084", "P1085", "P1087", "P1089", "P1090",
        "P1091", "P1099", "P1100", "P1102", "P1105", "P1106", "P1107", "P1110", "P1114", "P1115", "P1117",
        "P1126", "P1134",
    ];
    let cases: &[(&str, &str, &[&str])] = &[
        ("8.5", "<?php\nfunction assert($x) { return $x; }\n", &["P1100 Defining a function named \"assert\" is not allowed."]),
        ("8.5", "<?php\nclass H3 { public static private(set) int $s; }\n", &[]),
        ("8.5", "<?php\nclass H2 { public private(set) $u; }\n", &["P1067 A property with asymmetric visibility must have a type."]),
        ("8.5", "<?php\nclass H4 { private public(set) int $t; }\n", &["P1068 Set visibility must be equal to or less than get visibility."]),
        ("8.5", "<?php\n#[Attribute(Attribute::TARGET_CLASS)] class OnlyClass {} #[OnlyClass] function at() {}\n", &["P1087 Attribute \"OnlyClass\" has invalid target."]),
        ("8.5", "<?php\n$k = `ls`;\n", &["P1085 The backtick (`) operator is deprecated, use shell_exec() instead"]),
        ("8.5", "<?php\nfunction r(&$x) { $x = 1; } r(1);\n", &["P1114 Only variables should be passed by reference."]),
        ("8.5", "<?php\nclass C1 { case X; }\n", &["P1049 Enum case can only be used in an enum."]),
        ("8.5", "<?php\n$a = 1; switch ($a) { case 1; break; }\n", &["P1083 Case statements followed by a semicolon (;) are deprecated, use a colon (:) instead."]),
        ("8.5", "<?php\n$g = (integer) 1; $h = (boolean) 1; $i = (double) 1; $j = (binary) 1;\n", &["P1084 Non-canonical cast (binary) is deprecated. Use (string) instead.", "P1084 Non-canonical cast (boolean) is deprecated. Use (bool) instead.", "P1084 Non-canonical cast (double) is deprecated. Use (float) instead.", "P1084 Non-canonical cast (integer) is deprecated. Use (int) instead."]),
        ("8.5", "<?php\nclass Ca extends Cb {} class Cb extends Ca {}\n", &["P1134 'Ca' has a circular reference.", "P1134 'Cb' has a circular reference."]),
        ("8.5", "<?php\nclass CC { const F = function () {}; }\n", &["P1090 Closures in constant expressions must be static."]),
        ("8.5", "<?php\ntrait TP { public int $x = 1; } class CP { use TP; public string $x = 'a'; }\n", &["P1110 In the composition of type 'CP', properties 'CP::$x' and 'TP::$x' are not compatible."]),
        ("8.5", "<?php\nfunction foo() { return 1; } class A1 { const Z = [1, foo()]; }\n", &["P1089 Constant expression contains invalid operations."]),
        ("8.5", "<?php\nclass K1 { const int A = 1; } class K2 extends K1 { const string A = 'x'; }\n", &["P1102 Class constant K2::A is not compatible with K1::A."]),
        ("8.5", "<?php\n$d = 'x'; $e = $d{0};\n", &["P1034 Array or string offset access with curly braces deprecated in PHP 7.4. Targeting PHP 8.5."]),
        ("8.5", "<?php\n$d = 'x'; $f = \"a ${d} b\"; $g = \"a ${d['x']} b\";\n", &["P1063 Using ${} in strings is deprecated.", "P1063 Using ${} in strings is deprecated."]),
        ("8.5", "<?php\nfunction t2(int|int $a) { return $a; }\n", &["P1065 Redundant type."]),
        ("8.5", "<?php\n[] = [1];\n", &["P1025 Empty array destructure."]),
        ("8.5", "<?php\nenum E2: int { case A = 'x'; }\n", &[]),
        ("8.5", "<?php\nenum E1: float { case A = 1.0; }\n", &["P1051 Enum backing type must be int or string."]),
        ("8.5", "<?php\nfunction die() {}\n", &["P1100 Defining a function named \"die\" is not allowed."]),
        ("8.5", "<?php\nfunction exit() {}\n", &["P1100 Defining a function named \"exit\" is not allowed."]),
        ("8.5", "<?php\nclass HS { public int $x { set(int $a, int $b) {} } }\n", &["P1074 Invalid property hook signature."]),
        ("8.5", "<?php\nclass HU { public int $x { set => $value; } } $h = new HU(); unset($h->x);\n", &["P1076 Cannot unset hooked property HU::$x."]),
        ("8.5", "<?php\ninterface I1 { public int $x; }\n", &["P1082 Interfaces may only contain hooked properties."]),
        ("8.5", "<?php\n#[\\NoDiscard] function nd(): int { return 1; } nd();\n", &["P1126 The return value of nd() is expected to be consumed."]),
        ("8.5", "<?php\nnamespace N;\nfunction assert($x) { return $x; }\n", &["P1100 Defining a function named \"assert\" is not allowed."]),
        ("8.5", "<?php\necho 1;\nnamespace Foo;\n", &["P1105 Namespace declaration must be the first statement in script or immediately after declare statement."]),
        ("8.5", "<?php\nnamespace A { }\nnamespace B;\n", &["P1106 Cannot mix bracketed and unbracketed namespace declarations."]),
        ("8.5", "<?php\nnamespace A { }\necho 1;\n", &["P1107 No code may exist outside of bracketed namespace."]),
        ("8.5", "<?php\nfunction t1(null $a) { return $a; }\n", &[]),
        ("8.5", "<?php\n$a = (real) 1;\n", &["P1031 '(real)' cast deprecated in PHP 7.4. Targeting PHP 8.5."]),
        ("8.5", "<?php\nfunction t3(int|mixed $a) { return $a; }\n", &[]),
        ("8.5", "<?php\nclass Iterable {}\n", &["P1023 'Iterable' is a reserved word and cannot be used to name a class, interface, trait or namespace."]),
        ("8.5", "<?php\nreadonly class RC {} class NotRo extends RC {}\n", &["P1059 Non-readonly class cannot extend readonly class."]),
        ("8.5", "<?php\nclass Plain {} readonly class Ro2 extends Plain {}\n", &["P1060 Readonly class cannot extend non-readonly class."]),
        ("8.5", "<?php\nclass Readonly1 { readonly function m() {} }\n", &["P1045 Cannot use 'readonly' as method modifier."]),
        ("8.5", "<?php\nclass P1 { public readonly int $p; } class P2 extends P1 { public int $p; }\n", &["P1061 Cannot redeclare readonly property as non-readonly."]),
        ("8.5", "<?php\nreadonly class RC { public static int $s; }\n", &["P1058 Readonly class cannot declare static properties."]),
        ("8.5", "<?php\nclass SV { public private(set) int $x { get => 1; } }\n", &["P1071 Cannot specify set visibility for a get only virtual property $x."]),
        ("8.5", "<?php\nfunction sa(): null|false { return null; }\n", &[]),
        ("8.5", "<?php\nclass S1 { static const X = 1; }\n", &[]),
        ("8.5", "<?php\n$s = 'abc'; $s[] = 'd';\n", &["P1115 Cannot use [] operator on string."]),
        ("8.5", "<?php\n$c = 1 ? 2 : 3 ? 4 : 5;\n", &["P1033 Nested ternary expressions (without parentheses) deprecated in PHP 7.4. Targeting PHP 8.5."]),
        ("8.5", "<?php\n$a = 1 ?: 2 ?: 3;\n$b = (1 ? 2 : 3) ? 4 : 5;\n$c = 1 ? 2 : (3 ? 4 : 5);\n$d = 1 ? (2 ? 3 : 4) : 5;\n$e = 1 ? 2 ? 3 : 4 : 5;\n", &["P1033 Nested ternary expressions (without parentheses) deprecated in PHP 7.4. Targeting PHP 8.5."]),
        ("8.5", "<?php\ntrait T { const X = 1; }\n", &[]),
        ("8.5", "<?php\nclass TC { public void $x; }\n", &["P1048 Type 'void' cannot be used in this context."]),
        ("8.5", "<?php\n$b = (unset) 1;\n", &["P1032 '(unset)' cast deprecated in PHP 7.2. Targeting PHP 8.5."]),
        ("8.5", "<?php\n$v = (void) 1;\n", &["P1117 (void) cast cannot be used in this context."]),
        ("8.5", "<?php\nfunction t4(): void|int { return 1; }\n", &["P1048 Type 'void' cannot be used in this context."]),
        ("8.4", "<?php\nfunction assert($x) { return $x; }\n", &["P1100 Defining a function named \"assert\" is not allowed."]),
        ("8.4", "<?php\nclass H3 { public static private(set) int $s; }\n", &["P1069 Static property cannot have asymmetric visibility."]),
        ("8.4", "<?php\nclass H2 { public private(set) $u; }\n", &["P1067 A property with asymmetric visibility must have a type."]),
        ("8.4", "<?php\nclass H4 { private public(set) int $t; }\n", &["P1068 Set visibility must be equal to or less than get visibility."]),
        ("8.4", "<?php\n#[Attribute(Attribute::TARGET_CLASS)] class OnlyClass {} #[OnlyClass] function at() {}\n", &["P1087 Attribute \"OnlyClass\" has invalid target."]),
        ("8.4", "<?php\nfunction r(&$x) { $x = 1; } r(1);\n", &["P1114 Only variables should be passed by reference."]),
        ("8.4", "<?php\nclass C1 { case X; }\n", &["P1049 Enum case can only be used in an enum."]),
        ("8.4", "<?php\nclass Ca extends Cb {} class Cb extends Ca {}\n", &["P1134 'Ca' has a circular reference.", "P1134 'Cb' has a circular reference."]),
        ("8.4", "<?php\nclass CC { const F = function () {}; }\n", &["P1089 Constant expression contains invalid operations."]),
        ("8.4", "<?php\ntrait TP { public int $x = 1; } class CP { use TP; public string $x = 'a'; }\n", &["P1110 In the composition of type 'CP', properties 'CP::$x' and 'TP::$x' are not compatible."]),
        ("8.4", "<?php\nfunction foo() { return 1; } class A1 { const Z = [1, foo()]; }\n", &["P1089 Constant expression contains invalid operations."]),
        ("8.4", "<?php\nclass K1 { const int A = 1; } class K2 extends K1 { const string A = 'x'; }\n", &["P1102 Class constant K2::A is not compatible with K1::A."]),
        ("8.4", "<?php\n$d = 'x'; $e = $d{0};\n", &["P1034 Array or string offset access with curly braces deprecated in PHP 7.4. Targeting PHP 8.4."]),
        ("8.4", "<?php\n$d = 'x'; $f = \"a ${d} b\"; $g = \"a ${d['x']} b\";\n", &["P1063 Using ${} in strings is deprecated.", "P1063 Using ${} in strings is deprecated."]),
        ("8.4", "<?php\nfunction t2(int|int $a) { return $a; }\n", &["P1065 Redundant type."]),
        ("8.4", "<?php\n[] = [1];\n", &["P1025 Empty array destructure."]),
        ("8.4", "<?php\nenum E1: float { case A = 1.0; }\n", &["P1051 Enum backing type must be int or string."]),
        ("8.4", "<?php\nfunction die() {}\n", &["P1100 Defining a function named \"die\" is not allowed."]),
        ("8.4", "<?php\nfunction exit() {}\n", &["P1100 Defining a function named \"exit\" is not allowed."]),
        ("8.4", "<?php\nclass HS { public int $x { set(int $a, int $b) {} } }\n", &["P1074 Invalid property hook signature."]),
        ("8.4", "<?php\nclass HU { public int $x { set => $value; } } $h = new HU(); unset($h->x);\n", &["P1076 Cannot unset hooked property HU::$x."]),
        ("8.4", "<?php\ninterface I1 { public int $x; }\n", &["P1082 Interfaces may only contain hooked properties."]),
        ("8.4", "<?php\nnamespace N;\nfunction assert($x) { return $x; }\n", &["P1100 Defining a function named \"assert\" is not allowed."]),
        ("8.4", "<?php\necho 1;\nnamespace Foo;\n", &["P1105 Namespace declaration must be the first statement in script or immediately after declare statement."]),
        ("8.4", "<?php\nnamespace A { }\nnamespace B;\n", &["P1106 Cannot mix bracketed and unbracketed namespace declarations."]),
        ("8.4", "<?php\nnamespace A { }\necho 1;\n", &["P1107 No code may exist outside of bracketed namespace."]),
        ("8.4", "<?php\n$a = (real) 1;\n", &["P1031 '(real)' cast deprecated in PHP 7.4. Targeting PHP 8.4."]),
        ("8.4", "<?php\nclass Iterable {}\n", &["P1023 'Iterable' is a reserved word and cannot be used to name a class, interface, trait or namespace."]),
        ("8.4", "<?php\nreadonly class RC {} class NotRo extends RC {}\n", &["P1059 Non-readonly class cannot extend readonly class."]),
        ("8.4", "<?php\nclass Plain {} readonly class Ro2 extends Plain {}\n", &["P1060 Readonly class cannot extend non-readonly class."]),
        ("8.4", "<?php\nclass Readonly1 { readonly function m() {} }\n", &["P1045 Cannot use 'readonly' as method modifier."]),
        ("8.4", "<?php\nclass P1 { public readonly int $p; } class P2 extends P1 { public int $p; }\n", &["P1061 Cannot redeclare readonly property as non-readonly."]),
        ("8.4", "<?php\nreadonly class RC { public static int $s; }\n", &["P1058 Readonly class cannot declare static properties."]),
        ("8.4", "<?php\nclass SV { public private(set) int $x { get => 1; } }\n", &["P1071 Cannot specify set visibility for a get only virtual property $x."]),
        ("8.4", "<?php\n$s = 'abc'; $s[] = 'd';\n", &["P1115 Cannot use [] operator on string."]),
        ("8.4", "<?php\n$c = 1 ? 2 : 3 ? 4 : 5;\n", &["P1033 Nested ternary expressions (without parentheses) deprecated in PHP 7.4. Targeting PHP 8.4."]),
        ("8.4", "<?php\n$a = 1 ?: 2 ?: 3;\n$b = (1 ? 2 : 3) ? 4 : 5;\n$c = 1 ? 2 : (3 ? 4 : 5);\n$d = 1 ? (2 ? 3 : 4) : 5;\n$e = 1 ? 2 ? 3 : 4 : 5;\n", &["P1033 Nested ternary expressions (without parentheses) deprecated in PHP 7.4. Targeting PHP 8.4."]),
        ("8.4", "<?php\nclass TC { public void $x; }\n", &["P1048 Type 'void' cannot be used in this context."]),
        ("8.4", "<?php\n$b = (unset) 1;\n", &["P1032 '(unset)' cast deprecated in PHP 7.2. Targeting PHP 8.4."]),
        ("8.4", "<?php\n$v = (void) 1;\n", &["P1022 PHP 8.5+ syntax found. Targeting PHP 8.4."]),
        ("8.4", "<?php\nfunction t4(): void|int { return 1; }\n", &["P1048 Type 'void' cannot be used in this context."]),
        ("8.0", "<?php\nfunction assert($x) { return $x; }\n", &["P1100 Defining a function named \"assert\" is not allowed."]),
        ("8.0", "<?php\nclass H3 { public static private(set) int $s; }\n", &["P1022 PHP 8.4+ syntax found. Targeting PHP 8.0."]),
        ("8.0", "<?php\nclass H2 { public private(set) $u; }\n", &["P1022 PHP 8.4+ syntax found. Targeting PHP 8.0."]),
        ("8.0", "<?php\nclass H4 { private public(set) int $t; }\n", &["P1022 PHP 8.4+ syntax found. Targeting PHP 8.0."]),
        ("8.0", "<?php\n#[Attribute(Attribute::TARGET_CLASS)] class OnlyClass {} #[OnlyClass] function at() {}\n", &["P1087 Attribute \"OnlyClass\" has invalid target."]),
        ("8.0", "<?php\nfunction r(&$x) { $x = 1; } r(1);\n", &["P1114 Only variables should be passed by reference."]),
        ("8.0", "<?php\nclass C1 { case X; }\n", &["P1049 Enum case can only be used in an enum."]),
        ("8.0", "<?php\nclass Ca extends Cb {} class Cb extends Ca {}\n", &["P1134 'Ca' has a circular reference.", "P1134 'Cb' has a circular reference."]),
        ("8.0", "<?php\nclass CC { const F = function () {}; }\n", &["P1089 Constant expression contains invalid operations."]),
        ("8.0", "<?php\ntrait TP { public int $x = 1; } class CP { use TP; public string $x = 'a'; }\n", &["P1110 In the composition of type 'CP', properties 'CP::$x' and 'TP::$x' are not compatible."]),
        ("8.0", "<?php\nfunction foo() { return 1; } class A1 { const Z = [1, foo()]; }\n", &["P1089 Constant expression contains invalid operations."]),
        ("8.0", "<?php\nclass K1 { const int A = 1; } class K2 extends K1 { const string A = 'x'; }\n", &["P1022 PHP 8.3+ syntax found. Targeting PHP 8.0.", "P1022 PHP 8.3+ syntax found. Targeting PHP 8.0."]),
        ("8.0", "<?php\n$d = 'x'; $e = $d{0};\n", &["P1034 Array or string offset access with curly braces deprecated in PHP 7.4. Targeting PHP 8.0."]),
        ("8.0", "<?php\nfunction t2(int|int $a) { return $a; }\n", &["P1065 Redundant type."]),
        ("8.0", "<?php\n[] = [1];\n", &["P1025 Empty array destructure."]),
        ("8.0", "<?php\nfunction die() {}\n", &["P1100 Defining a function named \"die\" is not allowed."]),
        ("8.0", "<?php\nfunction exit() {}\n", &["P1100 Defining a function named \"exit\" is not allowed."]),
        ("8.0", "<?php\nclass HS { public int $x { set(int $a, int $b) {} } }\n", &["P1022 PHP 8.4+ syntax found. Targeting PHP 8.0."]),
        ("8.0", "<?php\nclass HU { public int $x { set => $value; } } $h = new HU(); unset($h->x);\n", &["P1022 PHP 8.4+ syntax found. Targeting PHP 8.0.", "P1076 Cannot unset hooked property HU::$x."]),
        ("8.0", "<?php\ninterface I1 { public int $x; }\n", &["P1082 Interfaces may only contain hooked properties."]),
        ("8.0", "<?php\nnamespace N;\nfunction assert($x) { return $x; }\n", &["P1100 Defining a function named \"assert\" is not allowed."]),
        ("8.0", "<?php\necho 1;\nnamespace Foo;\n", &["P1105 Namespace declaration must be the first statement in script or immediately after declare statement."]),
        ("8.0", "<?php\nnamespace A { }\nnamespace B;\n", &["P1106 Cannot mix bracketed and unbracketed namespace declarations."]),
        ("8.0", "<?php\nnamespace A { }\necho 1;\n", &["P1107 No code may exist outside of bracketed namespace."]),
        ("8.0", "<?php\nfunction t1(null $a) { return $a; }\n", &["P1064 'null' cannot be used as a standalone type."]),
        ("8.0", "<?php\n$a = (real) 1;\n", &["P1031 '(real)' cast deprecated in PHP 7.4. Targeting PHP 8.0."]),
        ("8.0", "<?php\nclass Iterable {}\n", &["P1023 'Iterable' is a reserved word and cannot be used to name a class, interface, trait or namespace."]),
        ("8.0", "<?php\nclass SV { public private(set) int $x { get => 1; } }\n", &["P1022 PHP 8.4+ syntax found. Targeting PHP 8.0.", "P1022 PHP 8.4+ syntax found. Targeting PHP 8.0."]),
        ("8.0", "<?php\n$s = 'abc'; $s[] = 'd';\n", &["P1115 Cannot use [] operator on string."]),
        ("8.0", "<?php\n$c = 1 ? 2 : 3 ? 4 : 5;\n", &["P1033 Nested ternary expressions (without parentheses) deprecated in PHP 7.4. Targeting PHP 8.0."]),
        ("8.0", "<?php\n$a = 1 ?: 2 ?: 3;\n$b = (1 ? 2 : 3) ? 4 : 5;\n$c = 1 ? 2 : (3 ? 4 : 5);\n$d = 1 ? (2 ? 3 : 4) : 5;\n$e = 1 ? 2 ? 3 : 4 : 5;\n", &["P1033 Nested ternary expressions (without parentheses) deprecated in PHP 7.4. Targeting PHP 8.0."]),
        ("8.0", "<?php\ntrait T { const X = 1; }\n", &["P1055 Traits cannot have constants."]),
        ("8.0", "<?php\nclass TC { public void $x; }\n", &["P1048 Type 'void' cannot be used in this context."]),
        ("8.0", "<?php\n$b = (unset) 1;\n", &["P1032 '(unset)' cast deprecated in PHP 7.2. Targeting PHP 8.0."]),
        ("8.0", "<?php\n$v = (void) 1;\n", &["P1022 PHP 8.5+ syntax found. Targeting PHP 8.0."]),
        ("8.0", "<?php\nfunction t4(): void|int { return 1; }\n", &["P1048 Type 'void' cannot be used in this context."]),
        ("7.4", "<?php\nfunction assert($x) { return $x; }\n", &["P1099 Defining a custom assert() function is deprecated."]),
        ("7.4", "<?php\nclass H3 { public static private(set) int $s; }\n", &["P1022 PHP 8.4+ syntax found. Targeting PHP 7.4."]),
        ("7.4", "<?php\nclass H2 { public private(set) $u; }\n", &["P1022 PHP 8.4+ syntax found. Targeting PHP 7.4."]),
        ("7.4", "<?php\nclass H4 { private public(set) int $t; }\n", &["P1022 PHP 8.4+ syntax found. Targeting PHP 7.4."]),
        ("7.4", "<?php\nfunction r(&$x) { $x = 1; } r(1);\n", &["P1114 Only variables should be passed by reference."]),
        ("7.4", "<?php\nclass C1 { case X; }\n", &["P1049 Enum case can only be used in an enum."]),
        ("7.4", "<?php\nclass Ca extends Cb {} class Cb extends Ca {}\n", &["P1134 'Ca' has a circular reference.", "P1134 'Cb' has a circular reference."]),
        ("7.4", "<?php\nclass CC { const F = function () {}; }\n", &["P1089 Constant expression contains invalid operations."]),
        ("7.4", "<?php\ntrait TP { public int $x = 1; } class CP { use TP; public string $x = 'a'; }\n", &["P1110 In the composition of type 'CP', properties 'CP::$x' and 'TP::$x' are not compatible."]),
        ("7.4", "<?php\nfunction foo() { return 1; } class A1 { const Z = [1, foo()]; }\n", &["P1089 Constant expression contains invalid operations."]),
        ("7.4", "<?php\nclass K1 { const int A = 1; } class K2 extends K1 { const string A = 'x'; }\n", &["P1022 PHP 8.3+ syntax found. Targeting PHP 7.4.", "P1022 PHP 8.3+ syntax found. Targeting PHP 7.4."]),
        ("7.4", "<?php\n$d = 'x'; $e = $d{0};\n", &["P1034 Array or string offset access with curly braces deprecated in PHP 7.4. Targeting PHP 7.4."]),
        ("7.4", "<?php\nfunction t2(int|int $a) { return $a; }\n", &["P1022 PHP 8.0+ syntax found. Targeting PHP 7.4."]),
        ("7.4", "<?php\n[] = [1];\n", &["P1025 Empty array destructure."]),
        ("7.4", "<?php\nfunction die() {}\n", &["P1100 Defining a function named \"die\" is not allowed."]),
        ("7.4", "<?php\nfunction exit() {}\n", &["P1100 Defining a function named \"exit\" is not allowed."]),
        ("7.4", "<?php\nclass HS { public int $x { set(int $a, int $b) {} } }\n", &["P1022 PHP 8.4+ syntax found. Targeting PHP 7.4."]),
        ("7.4", "<?php\nclass HU { public int $x { set => $value; } } $h = new HU(); unset($h->x);\n", &["P1022 PHP 8.4+ syntax found. Targeting PHP 7.4.", "P1076 Cannot unset hooked property HU::$x."]),
        ("7.4", "<?php\ninterface I1 { public int $x; }\n", &["P1082 Interfaces may only contain hooked properties."]),
        ("7.4", "<?php\nnamespace N;\nfunction assert($x) { return $x; }\n", &["P1099 Defining a custom assert() function is deprecated."]),
        ("7.4", "<?php\necho 1;\nnamespace Foo;\n", &["P1105 Namespace declaration must be the first statement in script or immediately after declare statement."]),
        ("7.4", "<?php\nnamespace A { }\nnamespace B;\n", &["P1106 Cannot mix bracketed and unbracketed namespace declarations."]),
        ("7.4", "<?php\nnamespace A { }\necho 1;\n", &["P1107 No code may exist outside of bracketed namespace."]),
        ("7.4", "<?php\nfunction t1(null $a) { return $a; }\n", &["P1022 PHP 8.2+ syntax found. Targeting PHP 7.4."]),
        ("7.4", "<?php\n$a = (real) 1;\n", &["P1031 '(real)' cast deprecated in PHP 7.4. Targeting PHP 7.4."]),
        ("7.4", "<?php\nfunction t3(int|mixed $a) { return $a; }\n", &["P1022 PHP 8.0+ syntax found. Targeting PHP 7.4."]),
        ("7.4", "<?php\nclass Iterable {}\n", &["P1023 'Iterable' is a reserved word and cannot be used to name a class, interface, trait or namespace."]),
        ("7.4", "<?php\nclass SV { public private(set) int $x { get => 1; } }\n", &["P1022 PHP 8.4+ syntax found. Targeting PHP 7.4.", "P1022 PHP 8.4+ syntax found. Targeting PHP 7.4."]),
        ("7.4", "<?php\nfunction sa(): null|false { return null; }\n", &["P1022 PHP 8.0+ syntax found. Targeting PHP 7.4."]),
        ("7.4", "<?php\n$s = 'abc'; $s[] = 'd';\n", &["P1115 Cannot use [] operator on string."]),
        ("7.4", "<?php\n$c = 1 ? 2 : 3 ? 4 : 5;\n", &["P1033 Nested ternary expressions (without parentheses) deprecated in PHP 7.4. Targeting PHP 7.4."]),
        ("7.4", "<?php\n$a = 1 ?: 2 ?: 3;\n$b = (1 ? 2 : 3) ? 4 : 5;\n$c = 1 ? 2 : (3 ? 4 : 5);\n$d = 1 ? (2 ? 3 : 4) : 5;\n$e = 1 ? 2 ? 3 : 4 : 5;\n", &["P1033 Nested ternary expressions (without parentheses) deprecated in PHP 7.4. Targeting PHP 7.4."]),
        ("7.4", "<?php\ntrait T { const X = 1; }\n", &["P1055 Traits cannot have constants."]),
        ("7.4", "<?php\nclass TC { public void $x; }\n", &["P1048 Type 'void' cannot be used in this context."]),
        ("7.4", "<?php\n$b = (unset) 1;\n", &["P1032 '(unset)' cast deprecated in PHP 7.2. Targeting PHP 7.4."]),
        ("7.4", "<?php\n$v = (void) 1;\n", &["P1022 PHP 8.5+ syntax found. Targeting PHP 7.4."]),
        ("7.4", "<?php\nfunction t4(): void|int { return 1; }\n", &["P1022 PHP 8.0+ syntax found. Targeting PHP 7.4."]),
    ];
    let mut failures = Vec::new();
    let mut workspaces: std::collections::HashMap<&str, Workspace> = std::collections::HashMap::new();
    for (version, text, expected) in cases {
        let ws = workspaces.entry(version).or_insert_with(|| {
            let mut ws =
                Workspace::new(Config { php_version: version.parse().unwrap(), ..Default::default() });
            ws.load_stubs();
            ws
        });
        let id = ws.set_file("file:///c.php", text.to_string());
        let mut got: Vec<String> = ws
            .diagnostics(id)
            .into_iter()
            .filter(|d| CODES.contains(&d.code.as_str()))
            .map(|d| format!("{} {}", d.code.as_str(), d.message))
            .collect();
        got.sort();
        if got != *expected {
            failures.push(format!(
                "{version} {text:?}
  expected {expected:?}
  got      {got:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}",
        failures.join(
            "
"
        )
    );
}

#[test]
fn twig_templates() {
    let (mut ws, dir) = project(
        "twig",
        &[
            ("composer.json", "{}"),
            ("symfony.lock", "{}"),
            (
                "src/Entity/User.php",
                "<?php\nnamespace App\\Entity;\nclass User {\n    public string $email = '';\n    /** The display name. */\n    public function getName(): string { return ''; }\n    public function isActive(): bool { return true; }\n    /** @return Post[] */\n    public function getPosts(): array { return []; }\n    public function rename(string $n): static { return $this; }\n}\nclass Post { public string $title = ''; }\n",
            ),
            (
                "src/Controller/UserController.php",
                "<?php\nnamespace App\\Controller;\nuse App\\Entity\\User;\nclass UserController extends \\Symfony\\Bundle\\FrameworkBundle\\Controller\\AbstractController {\n    public function show(User $user) {\n        return $this->render('user/show.html.twig', ['user' => $user, 'count' => 3]);\n    }\n    #[\\Route('/users/{id}', name: 'user_show')]\n    public function route() {}\n}\n",
            ),
            ("templates/base.html.twig", "<html>\n{% block body %}{% endblock %}\n{% block title %}Site{% endblock %}\n</html>\n"),
        ],
    );
    let path = "templates/user/show.html.twig";
    let template = "{% extends 'base.html.twig' %}\n{% block body %}\n  <h1>{{ user.name|upper }}</h1>\n  {% set total = user.posts|length %}\n  {% for post in user.posts %}\n    {{ post.title }} {{ loop.index }}\n  {% endfor %}\n  {# a comment\n     on two lines #}\n  <a href=\"{{ path('user_show') }}\">{{ count }} {{ total }}</a>\n{% endblock %}\n";
    let key = format!("file://{}", dir.join(path).display());
    std::fs::create_dir_all(dir.join("templates/user")).unwrap();
    std::fs::write(dir.join(path), template).unwrap();
    let id = ws.set_file(&key, template.to_string());
    let at = |needle: &str, delta: usize| TextSize::from((template.find(needle).unwrap() + delta) as u32);
    let labels = |ws: &Workspace, text: &str, offset: usize| {
        let id = ws.file_id(&key).unwrap();
        let _ = text;
        ws.completions(id, TextSize::from(offset as u32))
            .items
            .into_iter()
            .map(|i| i.label)
            .collect::<Vec<_>>()
    };

    // no PHP diagnostics, no formatting
    assert!(ws.diagnostics(id).is_empty(), "{:?}", ws.diagnostics(id));
    assert!(ws.format(id, None, &Default::default()).unwrap().is_empty());

    // variables from the controller, attributes through getters
    let h = ws.hover(id, at("user.name", 1)).unwrap();
    assert!(h.markdown.contains("App\\Entity\\User $user"), "{}", h.markdown);
    let h = ws.hover(id, at("user.name", 6)).unwrap();
    assert!(
        h.markdown.contains("getName(): string") && h.markdown.contains("The display name."),
        "{}",
        h.markdown
    );
    let defs = ws.goto_definition(id, at("user.name", 6));
    assert_eq!(defs.len(), 1);
    assert!(ws.file(defs[0].file).unwrap().path.ends_with("User.php"));
    let h = ws.hover(id, at("post.title", 6)).unwrap();
    assert!(h.markdown.contains("string $title"), "{}", h.markdown);
    let h = ws.hover(id, at("{{ total", 4)).unwrap();
    assert!(h.markdown.contains("int $total"), "{}", h.markdown);
    let h = ws.hover(id, at("{{ count", 4)).unwrap();
    assert!(h.markdown.contains("int $count"), "{}", h.markdown);
    // a variable from the controller: definition is the array key
    let defs = ws.goto_definition(id, at("{{ count", 4));
    assert!(ws.file(defs[0].file).unwrap().path.ends_with("UserController.php"));

    // completion
    let mut edited = template.replacen("{{ post.title }}", "{{ post. }}", 1);
    let offset = edited.find("post. ").unwrap() + 5;
    ws.set_file(&key, edited.clone());
    let l = labels(&ws, &edited, offset);
    assert!(l.contains(&"title".to_string()), "{l:?}");
    edited = template.replacen("{{ user.name|upper }}", "{{ user. }}", 1);
    let offset = edited.find("user. ").unwrap() + 5;
    ws.set_file(&key, edited.clone());
    let l = labels(&ws, &edited, offset);
    for want in ["name", "active", "posts", "email", "rename"] {
        assert!(l.contains(&want.to_string()), "{want} {l:?}");
    }
    edited = template.replacen("{{ loop.index }}", "{{ loop. }}", 1);
    let offset = edited.find("loop. ").unwrap() + 5;
    ws.set_file(&key, edited.clone());
    assert!(labels(&ws, &edited, offset).contains(&"index0".to_string()));
    edited = template.replacen("|upper", "|up", 1);
    let offset = edited.find("|up").unwrap() + 3;
    ws.set_file(&key, edited.clone());
    assert!(labels(&ws, &edited, offset).contains(&"upper".to_string()));
    edited = template.replacen("  {% endfor %}", "  {% e %}", 1);
    let offset = edited.find("{% e %}").unwrap() + 4;
    ws.set_file(&key, edited.clone());
    assert_eq!(labels(&ws, &edited, offset)[0], "endfor");
    edited = template.replacen("{{ count }}", "{{ c }}", 1);
    let offset = edited.find("{{ c }}").unwrap() + 4;
    ws.set_file(&key, edited.clone());
    let l = labels(&ws, &edited, offset);
    assert!(
        l.contains(&"count".to_string())
            && l.contains(&"user".to_string())
            && l.contains(&"path".to_string()),
        "{l:?}"
    );
    edited = template.replacen("'base.html.twig'", "''", 1);
    let offset = edited.find("''").unwrap() + 1;
    ws.set_file(&key, edited.clone());
    assert_eq!(labels(&ws, &edited, offset), vec!["base.html.twig", "user/show.html.twig"]);
    ws.set_file(&key, template.to_string());

    // navigation: templates, parent blocks, routes
    let t = ws.string_targets(id, at("base.html", 2));
    assert!(t[0].0.ends_with("templates/base.html.twig"), "{t:?}");
    let t = ws.string_targets(id, at("block body", 7));
    assert!(t[0].0.ends_with("templates/base.html.twig") && t[0].1 == 1, "{t:?}");
    let t = ws.string_targets(id, at("user_show", 2));
    assert!(t[0].0.ends_with("UserController.php"), "{t:?}");
    let links = ws.document_links(id);
    assert_eq!(links.len(), 1);
    assert!(links[0].target.ends_with("base.html.twig"));

    // structure
    let syms = ws.document_symbols(id);
    assert_eq!(syms.len(), 1);
    assert_eq!(syms[0].name, "body");
    let folds = ws.folding_ranges(id);
    assert_eq!(folds.len(), 3, "{folds:?}");
    let tokens = ws.semantic_tokens(id, None);
    assert!(tokens.iter().any(|t| t.token_type == php_ide::SemanticTokenType::Property));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The remaining catalog codes (and exact ranges of earlier ones), compared
/// with the real Intelephense: codes, ranges (byte offsets) and messages.
#[test]
fn catalog_round_four() {
    const CODES: &[&str] = &[
        "P1024", "P1026", "P1027", "P1028", "P1040", "P1053", "P1054", "P1066", "P1070", "P1072", "P1073",
        "P1077", "P1081", "P1092", "P1095", "P1111", "P1112", "P1127", "P1129", "P1065", "P1038", "P1039",
        "P1050", "P1052", "P1086", "P1093", "P1096", "P1103", "P1041",
    ];
    let cases: &[(&str, &str, &[&str])] = &[
        ("7.4", "<?php\n/** @param callable(int): $x */\nfunction f($x) {}\n", &["P1129 35..37 PHPDoc parse error. Unexpected token."]),
        ("7.4", "<?php\n/** @return Foo<int, */\nfunction f() {}\n", &["P1129 27..29 PHPDoc parse error. Unexpected token."]),
        ("7.4", "<?php\n/** @param array< $x */\nfunction f($x) {}\n", &["P1129 27..29 PHPDoc parse error. Unexpected token."]),
        ("7.4", "<?php\n/** @param (int $x */\nfunction f($x) {}\n", &["P1129 22..24 PHPDoc parse error. Unexpected token."]),
        ("7.4", "<?php\n/** @property int< $p */\nclass A {}\n", &["P1129 28..30 PHPDoc parse error. Unexpected token."]),
        ("7.4", "<?php\n/** @return int| */\nfunction f() { return 1; }\n", &["P1129 23..25 PHPDoc parse error. Unexpected token."]),
        ("7.4", "<?php\n/** @param array{a: int $x */\nfunction f($x) {}\n", &["P1129 30..32 PHPDoc parse error. Unexpected token."]),
        ("7.4", "<?php\n/** @var { */\n$x = 1;\n", &["P1129 15..16 PHPDoc parse error. Unexpected token."]),
        ("7.4", "<?php\n$x = <<<EOT\n  a\n b\n  EOT;\n", &["P1027 18..24 Unexpected indentation level. Expected at least 2. Found 1."]),
        ("7.4", "<?php\n$x = <<<EOT\n \ta\n \tEOT;\n", &["P1028 21..27 Invalid indentation. Mixed spaces and tabs are not allowed."]),
        ("7.4", "<?php\n$x = <<<EOT\n\ta\n    EOT;\n", &["P1028 18..20 Invalid indentation. Mixed spaces and tabs are not allowed."]),
        ("7.4", "<?php\n$x = <<<'EOT'\n    a\n  b\n    EOT;\n", &["P1027 20..29 Unexpected indentation level. Expected at least 4. Found 2."]),
        ("7.4", "<?php\nnamespace Foo\\List;\n", &["P1040 16..24 Namespace names can only contain keywords in PHP 8+."]),
        ("7.4", "<?php\nnamespace Foo\\Fn\\Bar;\n", &["P1040 16..26 Namespace names can only contain keywords in PHP 8+."]),
        ("7.4", "<?php\n$a = [];\necho $a[];\n", &["P1026 20..24 Key expected in read context."]),
        ("7.4", "<?php\n$a = [];\n$x = $a[];\n", &["P1026 20..24 Key expected in read context."]),
        ("7.4", "<?php\nfunction f() { return 1; }\nf() = 1;\n", &["P1024 33..36 Expression is not writable."]),
        ("7.4", "<?php\nfunction f() { return 1; }\nf() .= 'x';\n", &["P1024 33..36 Expression is not writable."]),
        ("7.4", "<?php\nconst C = 1;\nC = 2;\n", &["P1024 19..20 Expression is not writable."]),
        ("7.4", "<?php\nfunction f() { return 1; }\n[f()] = [1];\n", &["P1024 34..37 Expression is not writable."]),
        ("7.4", "<?php\nfunction f() { return 1; }\nforeach ([1] as f()) {}\n", &["P1024 49..52 Expression is not writable."]),
        ("7.4", "<?php\nfunction f() { return 1; }\n++f();\n", &["P1024 35..38 Expression is not writable."]),
        ("7.4", "<?php\n'x' = 1;\n", &["P1024 6..9 Expression is not writable."]),
        ("8.0", "<?php\nfunction f(private(set) int $x) {}\n", &["P1041 17..36 Cannot declare property promotion outside of constructor."]),
        ("8.0", "<?php\n/** @param callable(int): $x */\nfunction f($x) {}\n", &["P1129 35..37 PHPDoc parse error. Unexpected token."]),
        ("8.0", "<?php\n/** @return Foo<int, */\nfunction f() {}\n", &["P1129 27..29 PHPDoc parse error. Unexpected token."]),
        ("8.0", "<?php\n/** @param array< $x */\nfunction f($x) {}\n", &["P1129 27..29 PHPDoc parse error. Unexpected token."]),
        ("8.0", "<?php\n/** @param (int $x */\nfunction f($x) {}\n", &["P1129 22..24 PHPDoc parse error. Unexpected token."]),
        ("8.0", "<?php\n/** @property int< $p */\nclass A {}\n", &["P1129 28..30 PHPDoc parse error. Unexpected token."]),
        ("8.0", "<?php\n/** @return int| */\nfunction f() { return 1; }\n", &["P1129 23..25 PHPDoc parse error. Unexpected token."]),
        ("8.0", "<?php\n/** @param array{a: int $x */\nfunction f($x) {}\n", &["P1129 30..32 PHPDoc parse error. Unexpected token."]),
        ("8.0", "<?php\n/** @var { */\n$x = 1;\n", &["P1129 15..16 PHPDoc parse error. Unexpected token."]),
        ("8.0", "<?php\ninterface A {} interface B {}\nfunction f((A&B)|(A&B) $a) {}\n", &["P1065 53..58 Redundant type."]),
        ("8.0", "<?php\nfunction f(?int $a, int|null|NULL $b) {}\n", &["P1065 35..39 Redundant type."]),
        ("8.0", "<?php\n$x = <<<EOT\n  a\n b\n  EOT;\n", &["P1027 18..24 Unexpected indentation level. Expected at least 2. Found 1."]),
        ("8.0", "<?php\n$x = <<<EOT\n \ta\n \tEOT;\n", &["P1028 21..27 Invalid indentation. Mixed spaces and tabs are not allowed."]),
        ("8.0", "<?php\n$x = <<<EOT\n\ta\n    EOT;\n", &["P1028 18..20 Invalid indentation. Mixed spaces and tabs are not allowed."]),
        ("8.0", "<?php\n$x = <<<'EOT'\n    a\n  b\n    EOT;\n", &["P1027 20..29 Unexpected indentation level. Expected at least 4. Found 2."]),
        ("8.0", "<?php\n$a = [];\necho $a[];\n", &["P1026 20..24 Key expected in read context."]),
        ("8.0", "<?php\n$a = [];\n$x = $a[];\n", &["P1026 20..24 Key expected in read context."]),
        ("8.0", "<?php\nfunction f() { return 1; }\nf() = 1;\n", &["P1024 33..36 Expression is not writable."]),
        ("8.0", "<?php\nfunction f() { return 1; }\nf() .= 'x';\n", &["P1024 33..36 Expression is not writable."]),
        ("8.0", "<?php\nconst C = 1;\nC = 2;\n", &["P1024 19..20 Expression is not writable."]),
        ("8.0", "<?php\nfunction f() { return 1; }\n[f()] = [1];\n", &["P1024 34..37 Expression is not writable."]),
        ("8.0", "<?php\nfunction f() { return 1; }\nforeach ([1] as f()) {}\n", &["P1024 49..52 Expression is not writable."]),
        ("8.0", "<?php\nfunction f() { return 1; }\n++f();\n", &["P1024 35..38 Expression is not writable."]),
        ("8.0", "<?php\n'x' = 1;\n", &["P1024 6..9 Expression is not writable."]),
        ("8.4", "<?php\nclass A { private(set) const X = 1; }\n", &["P1066 16..28 Cannot use private(set), protected(set), public(set) on non-property."]),
        ("8.4", "<?php\nclass A { private(set) function m() {} }\n", &["P1066 16..28 Cannot use private(set), protected(set), public(set) on non-property."]),
        ("8.4", "<?php\nfunction f(private(set) int $x) {}\n", &["P1041 17..36 Cannot declare property promotion outside of constructor."]),
        ("8.4", "<?php\n/** @param callable(int): $x */\nfunction f($x) {}\n", &["P1129 35..37 PHPDoc parse error. Unexpected token."]),
        ("8.4", "<?php\n/** @return Foo<int, */\nfunction f() {}\n", &["P1129 27..29 PHPDoc parse error. Unexpected token."]),
        ("8.4", "<?php\n/** @param array< $x */\nfunction f($x) {}\n", &["P1129 27..29 PHPDoc parse error. Unexpected token."]),
        ("8.4", "<?php\n/** @param (int $x */\nfunction f($x) {}\n", &["P1129 22..24 PHPDoc parse error. Unexpected token."]),
        ("8.4", "<?php\n/** @property int< $p */\nclass A {}\n", &["P1129 28..30 PHPDoc parse error. Unexpected token."]),
        ("8.4", "<?php\n/** @return int| */\nfunction f() { return 1; }\n", &["P1129 23..25 PHPDoc parse error. Unexpected token."]),
        ("8.4", "<?php\n/** @param array{a: int $x */\nfunction f($x) {}\n", &["P1129 30..32 PHPDoc parse error. Unexpected token."]),
        ("8.4", "<?php\n/** @var { */\n$x = 1;\n", &["P1129 15..16 PHPDoc parse error. Unexpected token."]),
        ("8.4", "<?php\ninterface A {} interface B {}\nfunction f((A&B)|(A&B) $a) {}\n", &["P1065 53..58 Redundant type."]),
        ("8.4", "<?php\ninterface A {}\nfunction f(A&A $a) {}\n", &["P1065 34..35 Redundant type."]),
        ("8.4", "<?php\nfunction f(?int $a, int|null|NULL $b) {}\n", &["P1065 35..39 Redundant type."]),
        ("8.4", "<?php\nclass A {}\n$f = new A(...);\n", &["P1053 28..31 Cannot create closure for new expression."]),
        ("8.4", "<?php\n$x = <<<EOT\n  a\n b\n  EOT;\n", &["P1027 18..24 Unexpected indentation level. Expected at least 2. Found 1."]),
        ("8.4", "<?php\n$x = <<<EOT\n \ta\n \tEOT;\n", &["P1028 21..27 Invalid indentation. Mixed spaces and tabs are not allowed."]),
        ("8.4", "<?php\n$x = <<<EOT\n\ta\n    EOT;\n", &["P1028 18..20 Invalid indentation. Mixed spaces and tabs are not allowed."]),
        ("8.4", "<?php\n$x = <<<'EOT'\n    a\n  b\n    EOT;\n", &["P1027 20..29 Unexpected indentation level. Expected at least 4. Found 2."]),
        ("8.4", "<?php\n$a = [];\necho $a[];\n", &["P1026 20..24 Key expected in read context."]),
        ("8.4", "<?php\n$a = [];\n$x = $a[];\n", &["P1026 20..24 Key expected in read context."]),
        ("8.4", "<?php\nclass A { public int $v = 5 { get => 1; } }\n", &["P1072 30..33 Virtual property $v cannot have a default value."]),
        ("8.4", "<?php\nclass A { public int $v = 5 { get { return 1; } } }\n", &["P1072 30..33 Virtual property $v cannot have a default value."]),
        ("8.4", "<?php\nfunction f() { return 1; }\nf() = 1;\n", &["P1024 33..36 Expression is not writable."]),
        ("8.4", "<?php\nfunction f() { return 1; }\nf() .= 'x';\n", &["P1024 33..36 Expression is not writable."]),
        ("8.4", "<?php\nconst C = 1;\nC = 2;\n", &["P1024 19..20 Expression is not writable."]),
        ("8.4", "<?php\nfunction f() { return 1; }\n[f()] = [1];\n", &["P1024 34..37 Expression is not writable."]),
        ("8.4", "<?php\nfunction f() { return 1; }\nforeach ([1] as f()) {}\n", &["P1024 49..52 Expression is not writable."]),
        ("8.4", "<?php\nfunction f() { return 1; }\n++f();\n", &["P1024 35..38 Expression is not writable."]),
        ("8.4", "<?php\n'x' = 1;\n", &["P1024 6..9 Expression is not writable."]),
        ("8.5", "<?php\nclass A { private(set) const X = 1; }\n", &["P1066 16..28 Cannot use private(set), protected(set), public(set) on non-property."]),
        ("8.5", "<?php\nclass A { private(set) function m() {} }\n", &["P1066 16..28 Cannot use private(set), protected(set), public(set) on non-property."]),
        ("8.5", "<?php\nfunction f(private(set) int $x) {}\n", &["P1041 17..36 Cannot declare property promotion outside of constructor."]),
        ("8.5", "<?php\n/** @param callable(int): $x */\nfunction f($x) {}\n", &["P1129 35..37 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/** @return Foo<int, */\nfunction f() {}\n", &["P1129 27..29 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/** @param array<int, string> $x\n * @return array{a: int, b?: string} */\nfunction f($x) { return []; }\n", &[]),
        ("8.5", "<?php\n/** @param array< $x */\nfunction f($x) {}\n", &["P1129 27..29 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/** @param (int $x */\nfunction f($x) {}\n", &["P1129 22..24 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/** @property int< $p */\nclass A {}\n", &["P1129 28..30 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/** @return int| */\nfunction f() { return 1; }\n", &["P1129 23..25 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/** @param array{a: int $x */\nfunction f($x) {}\n", &["P1129 30..32 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/** @var { */\n$x = 1;\n", &["P1129 15..16 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\ntry {} catch (Exception|Exception $e) {}\n", &[]),
        ("8.5", "<?php\ninterface A {} interface B {}\nfunction f((A&B)|(A&B) $a) {}\n", &["P1065 53..58 Redundant type."]),
        ("8.5", "<?php\ninterface A {}\nfunction f(A&A $a) {}\n", &["P1065 34..35 Redundant type."]),
        ("8.5", "<?php\nfunction f(?int $a, int|null|NULL $b) {}\n", &["P1065 35..39 Redundant type."]),
        ("8.5", "<?php\nclass A {}\n$f = new A(...);\n", &["P1053 28..31 Cannot create closure for new expression."]),
        ("8.5", "<?php\n$x = <<<EOT\n  a\n b\n  EOT;\n", &["P1027 18..24 Unexpected indentation level. Expected at least 2. Found 1."]),
        ("8.5", "<?php\n$x = <<<EOT\n \ta\n \tEOT;\n", &["P1028 21..27 Invalid indentation. Mixed spaces and tabs are not allowed."]),
        ("8.5", "<?php\n$x = <<<EOT\n\ta\n    EOT;\n", &["P1028 18..20 Invalid indentation. Mixed spaces and tabs are not allowed."]),
        ("8.5", "<?php\nclass A { public array $a { get => []; } }\n$o = new A();\n$o->a[] = 1;\n", &[]),
        ("8.5", "<?php\nclass A { public array $a { get => []; } }\n$o = new A();\n$o->a['k'] = 1;\n", &[]),
        ("8.5", "<?php\nclass A { public function __construct(public readonly array $a = []) {} }\n$o = new A();\n$o->a[] = 1;\n", &[]),
        ("8.5", "<?php\nclass A { #[\\NoDiscard] public function __construct() {} }\n", &["P1127 18..28 Attribute 'NoDiscard' cannot be used on magic method '__construct'."]),
        ("8.5", "<?php\n$x = <<<'EOT'\n    a\n  b\n    EOT;\n", &["P1027 20..29 Unexpected indentation level. Expected at least 4. Found 2."]),
        ("8.5", "<?php\n#[\\NoDiscard] function f(): never { exit; }\n", &["P1127 8..18 Attribute 'NoDiscard' cannot be used on functions or methods with a 'void' or 'never' return type."]),
        ("8.5", "<?php\nclass A { #[\\NoDiscard] public int $p = 1; }\n", &[]),
        ("8.5", "<?php\n#[\\NoDiscard] function f(): void {}\n", &["P1127 8..18 Attribute 'NoDiscard' cannot be used on functions or methods with a 'void' or 'never' return type."]),
        ("8.5", "<?php\nnamespace Foo\\List;\n", &[]),
        ("8.5", "<?php\nnamespace Foo\\Fn\\Bar;\n", &[]),
        ("8.5", "<?php\n$x = 1 |> fn($a) => $a;\n", &["P1092 16..28 Arrow functions on the right hand side of |> must be parenthesized."]),
        ("8.5", "<?php\n$x = 1 |> (fn($a) => $a);\n", &[]),
        ("8.5", "<?php\n$a = [];\necho $a[];\n", &["P1026 20..24 Key expected in read context."]),
        ("8.5", "<?php\n$a = [];\n$x = $a[];\n", &["P1026 20..24 Key expected in read context."]),
        ("8.5", "<?php\nclass S1 { static const X = 1; }\n", &[]),
        ("8.5", "<?php\nclass A { public int $x { static get => 1; } }\n", &[]),
        ("8.5", "<?php\ninterface I { static public int $x { get; } }\n", &[]),
        ("8.5", "<?php\nclass A { public int $v = 5 { get => 1; } }\n", &["P1072 30..33 Virtual property $v cannot have a default value."]),
        ("8.5", "<?php\nclass A { public int $v = 5 { get { return 1; } } }\n", &["P1072 30..33 Virtual property $v cannot have a default value."]),
        ("8.5", "<?php\nfunction f() { return 1; }\nf() = 1;\n", &["P1024 33..36 Expression is not writable."]),
        ("8.5", "<?php\nfunction f() { return 1; }\nf() .= 'x';\n", &["P1024 33..36 Expression is not writable."]),
        ("8.5", "<?php\nconst C = 1;\nC = 2;\n", &["P1024 19..20 Expression is not writable."]),
        ("8.5", "<?php\nfunction f() { return 1; }\n[f()] = [1];\n", &["P1024 34..37 Expression is not writable."]),
        ("8.5", "<?php\nfunction f() { return 1; }\nforeach ([1] as f()) {}\n", &["P1024 49..52 Expression is not writable."]),
        ("8.5", "<?php\nfunction f() { return 1; }\n++f();\n", &["P1024 35..38 Expression is not writable."]),
        ("8.5", "<?php\n'x' = 1;\n", &["P1024 6..9 Expression is not writable."]),
        ("8.5", "<?php\nfunction f() { return 1; }\n$x = &f();\n", &[]),
        ("8.4", "<?php\ninterface Ca { const X = 1; }\ninterface Cb { const X = 2; }\nclass Cc implements Ca, Cb {}\n", &["P1111 66..92 In the composition of type 'Cc', class constants 'Ca::X' and 'Cb::X' are not compatible."]),
        ("8.4", "<?php\ntrait Cd { public const X = 1; }\nclass Ce { public const X = 2; }\nclass Cf extends Ce { use Cd; }\n", &["P1111 72..91 In the composition of type 'Cf', class constants 'Cd::X' and 'Ce::X' are not compatible."]),
        ("8.4", "<?php\ntrait Cg { public const X = 1; }\nclass Ch { use Cg; protected const X = 1; }\n", &["P1111 74..79 In the composition of type 'Ch', class constants 'Ch::X' and 'Cg::X' are not compatible."]),
        ("8.4", "<?php\ninterface Dc {} interface Dd extends Dc, Dc {}\n", &["P1095 47..49 Duplicate type Dc in extends list."]),
        ("8.4", "<?php\ninterface Da {}\nclass Db implements Da, Da {}\n", &["P1095 46..48 Duplicate type Da in implements list."]),
        ("8.4", "<?php\ntrait De {}\nclass Df { use De, De; }\n", &["P1095 37..39 Duplicate type De in use list."]),
        ("8.4", "<?php\nclass Rg { public array $a { get => []; } }\nfunction g(Rg $o) { $r = &$o->a; }\n", &["P1073 70..81 Cannot indirectly modify property Rg::$a."]),
        ("8.4", "<?php\ntrait Ma { public function m(): int { return 1; } }\ninterface Mb { public function m(): string; }\nclass Mc implements Mb { use Ma; }\n", &["P1112 104..126 In the composition of type 'Mc', methods 'Ma::m()' and 'Mb::m()' are not compatible."]),
        ("8.4", "<?php\nclass Mj { public function m(): int { return 1; } }\ntrait Mk { public function m(): string { return ''; } }\nclass Ml extends Mj { use Mk; }\n", &["P1112 114..133 In the composition of type 'Ml', methods 'Mk::m()' and 'Mj::m()' are not compatible."]),
        ("8.4", "<?php\nclass Tc { public ?int $p = null; }\nclass Td extends Tc { public int $p = 0; }\n", &["P1077 64..82 Property Td::$p must have type null|int."]),
        ("8.4", "<?php\nfunction f(static $x) {}\n", &["P1041 17..26 Cannot declare property promotion outside of constructor.", "P1081 17..23 Static modifier not allowed."]),
        ("8.4", "<?php\nclass Sa { public function __construct(public static int $x) {} }\n", &["P1081 52..58 Static modifier not allowed."]),
        ("8.4", "<?php\nclass Vg { public public(set) int $p = 0; }\nclass Vh extends Vg { public private(set) int $p = 0; }\n", &["P1070 96..102 Visibility of 'Vh::$p' must be the same or less restrictive than 'Vg::$p'."]),
        ("8.4", "<?php\nclass Vc { public static $p; }\nclass Vd extends Vc { protected static $p; }\n", &["P1070 76..78 Visibility of 'Vd::$p' must be the same or less restrictive than 'Vc::$p'."]),
        ("8.5", "<?php\ninterface Ca { const X = 1; }\ninterface Cb { const X = 2; }\nclass Cc implements Ca, Cb {}\n", &["P1111 66..92 In the composition of type 'Cc', class constants 'Ca::X' and 'Cb::X' are not compatible."]),
        ("8.5", "<?php\ntrait Cd { public const X = 1; }\nclass Ce { public const X = 2; }\nclass Cf extends Ce { use Cd; }\n", &[]),
        ("8.5", "<?php\ntrait Cg { public const X = 1; }\nclass Ch { use Cg; protected const X = 1; }\n", &["P1111 74..79 In the composition of type 'Ch', class constants 'Ch::X' and 'Cg::X' are not compatible."]),
        ("8.5", "<?php\nclass Dg extends Exception {}\ntry {} catch (Dg|Dg $e) {}\n", &[]),
        ("8.5", "<?php\n/** @param int|int $x */\nfunction dh($x) {}\n", &[]),
        ("8.5", "<?php\ninterface Dc {} interface Dd extends Dc, Dc {}\n", &["P1095 47..49 Duplicate type Dc in extends list."]),
        ("8.5", "<?php\ninterface Da {}\nclass Db implements Da, Da {}\n", &["P1095 46..48 Duplicate type Da in implements list."]),
        ("8.5", "<?php\n/** @throws Exception|Exception */\nfunction di() {}\n", &[]),
        ("8.5", "<?php\ntrait De {}\nclass Df { use De, De; }\n", &["P1095 37..39 Duplicate type De in use list."]),
        ("8.5", "<?php\nclass Re { public array $a = [] { get => $this->a; set => $value; } }\nfunction g(Re $o) { $o->a[] = 1; }\n", &[]),
        ("8.5", "<?php\nclass Rh { public int $a { get => 1; } }\nfunction g(Rh $o) { $o->a++; }\n", &[]),
        ("8.5", "<?php\nclass Rf { public array $a { get => []; } }\nfunction g(Rf $o) { $o->a['x']['y'] = 1; }\n", &[]),
        ("8.5", "<?php\nclass Rg { public array $a { get => []; } }\nfunction g(Rg $o) { $r = &$o->a; }\n", &["P1073 70..81 Cannot indirectly modify property Rg::$a."]),
        ("8.5", "<?php\nclass Ra { public readonly array $a; public function __construct() { $this->a = []; } }\n$o = new Ra();\n$o->a[] = 1;\n", &[]),
        ("8.5", "<?php\nclass Rb { public readonly array $a; public function __construct() { $this->a = []; } public function f() { $this->a[] = 1; } }\n", &[]),
        ("8.5", "<?php\nclass Rd { public array $a { set => $value; } }\nfunction g(Rd $o) { $o->a['x'] = 1; }\n", &[]),
        ("8.5", "<?php\nclass Rc { public array $a { get => []; } }\nfunction g(Rc $o) { $o->a[] = 1; }\n", &[]),
        ("8.5", "<?php\ntrait Ma { public function m(): int { return 1; } }\ninterface Mb { public function m(): string; }\nclass Mc implements Mb { use Ma; }\n", &["P1112 104..126 In the composition of type 'Mc', methods 'Ma::m()' and 'Mb::m()' are not compatible."]),
        ("8.5", "<?php\nclass Mj { public function m(): int { return 1; } }\ntrait Mk { public function m(): string { return ''; } }\nclass Ml extends Mj { use Mk; }\n", &["P1112 114..133 In the composition of type 'Ml', methods 'Mk::m()' and 'Mj::m()' are not compatible."]),
        ("8.5", "<?php\ntrait Mg { abstract public function m(int $a): void; }\ntrait Mh { public function m(string $a): void {} }\nclass Mi { use Mg, Mh; }\n", &[]),
        ("8.5", "<?php\ninterface Md { public function m(): int; }\ninterface Me { public function m(): string; }\nabstract class Mf implements Md, Me {}\n", &[]),
        ("8.5", "<?php\nclass Te { public string $p { set(int $v) { } } }\n", &[]),
        ("8.5", "<?php\ninterface Tf { public int $p { set; } }\nclass Tg implements Tf { public ?int $p = 0; }\n", &[]),
        ("8.5", "<?php\nclass Tc { public ?int $p = null; }\nclass Td extends Tc { public int $p = 0; }\n", &["P1077 64..82 Property Td::$p must have type null|int."]),
        ("8.5", "<?php\ntrait Ta { public int $p = 0; }\nclass Tb { use Ta; public string $p = ''; }\n", &[]),
        ("8.5", "<?php\nclass Sd { static function __construct() {} }\n", &[]),
        ("8.5", "<?php\nstatic class Sb {}\n", &[]),
        ("8.5", "<?php\n$f = static fn() => 1;\n", &[]),
        ("8.5", "<?php\nenum Sf { case A; static abstract function m(); }\n", &[]),
        ("8.5", "<?php\nclass Se { public int $x { static set => $value; } }\n", &[]),
        ("8.5", "<?php\nclass Si { public static int $x { get => 1; } }\n", &[]),
        ("8.5", "<?php\ninterface Sc { static const X = 1; }\n", &[]),
        ("8.5", "<?php\nfunction f(static $x) {}\n", &["P1041 17..26 Cannot declare property promotion outside of constructor.", "P1081 17..23 Static modifier not allowed."]),
        ("8.5", "<?php\nclass Sa { public function __construct(public static int $x) {} }\n", &["P1081 52..58 Static modifier not allowed."]),
        ("8.5", "<?php\nclass Sh { static readonly int $x; }\n", &[]),
        ("8.5", "<?php\ntrait Sg { static public $x; }\n", &[]),
        ("8.5", "<?php\nclass Va { public const X = 1; }\nclass Vb extends Va { private const X = 2; }\n", &[]),
        ("8.5", "<?php\nclass Ve { public $p; }\nclass Vf extends Ve { public function __construct(protected $p) {} }\n", &[]),
        ("8.5", "<?php\nclass Vg { public public(set) int $p = 0; }\nclass Vh extends Vg { public private(set) int $p = 0; }\n", &["P1070 96..102 Visibility of 'Vh::$p' must be the same or less restrictive than 'Vg::$p'."]),
        ("8.5", "<?php\nclass Vc { public static $p; }\nclass Vd extends Vc { protected static $p; }\n", &["P1070 76..78 Visibility of 'Vd::$p' must be the same or less restrictive than 'Vc::$p'."]),
        ("8.5", "<?php\n/**\n * @param int||string $x\n */\nfunction f6($x) {}\n", &["P1129 24..25 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/**\n * @param Foo<int>> $x\n */\nfunction f5($x) {}\n", &[]),
        ("8.5", "<?php\n/**\n * @method int< foo()\n */\nclass A7 {}\n", &["P1129 29..31 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/**\n * Text.\n * @param array< $x the x\n * @return int\n */\nfunction f1($x) { return 1; }\n", &["P1129 39..42 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/**\n * @return int|\n */\nfunction f2() { return 1; }\n", &["P1129 27..29 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/**\n * @param array{a: int, b: $x\n */\nfunction f3($x) {}\n", &["P1129 41..43 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/**\n * @template T of array<\n */\nfunction f9() {}\n", &["P1129 36..38 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/**\n * @throws Exception|\n */\nfunction f8() {}\n", &["P1129 33..35 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\nclass A4 {\n    /** @var array<int, */\n    public $p;\n}\n", &["P1129 41..43 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/**\n * @template T\n * @param T $x\n * @return (T is string ? int : (T is int ? string : null))\n */\nfunction f14($x) { return null; }\n", &[]),
        ("8.5", "<?php\n/**\n * @param int $x Some (text with parens and <tags>\n * @param string $y {@see foo()}\n * @return array A list of {things}\n */\nfunction f12($x, $y) { return []; }\n", &[]),
        ("8.5", "<?php\n/**\n * @param $x\n * @return\n */\nfunction f15($x) {}\n", &["P1129 35..37 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/**\n * @param array<int, array{a: int, 'b c'?: string}> $a\n * @param callable(int, string): void $b\n * @param \\Closure(): int $c\n * @param int<0, max> $d\n * @param value-of<A10::*> $e\n * @param 'a'|'b'|\"c\" $f\n * @param list{int, string} $g\n * @param (A10|B10)[] $h\n * @param non-empty-array<int>|null $i\n * @param object{a: int} $j\n * @param array<string>[] $k\n * @param ?int $l\n * @param key-of<array<string, int>> $m\n * @param A10::CONST_* $n\n * @param callable-string $o\n * @param class-string<A10> $p\n * @param -1|1.5 $q\n * @param int-mask<1, 2> $r\n * @return ($a is int ? string : bool)\n */\nfunction f10($a, $b, $c, $d, $e, $f, $g, $h, $i, $j, $k, $l, $m, $n, $o, $p, $q, $r) { return true; }\nclass A10 { const CONST_A = 1; } class B10 {}\n", &[]),
        ("8.5", "<?php\n/**\n * @template-covariant T of object\n * @extends \\ArrayObject<int, T>\n * @phpstan-param array<T> $x\n * @psalm-return iterable<int, T>\n */\nclass C11 extends \\ArrayObject { /** @param T $x @return static */ function f($x) { return $this; } }\n", &[]),
        ("8.5", "<?php\n/**\n * @param array< int , string > $x\n * @param array{ a : int } $y\n */\nfunction f16($x, $y) {}\n", &[]),
        ("8.5", "<?php\nfunction f13() { /** @var array<int, string> $x */ $x = []; /** @var \\stdClass */ $y = new \\stdClass; return [$x, $y]; }\n", &[]),
        ("8.5", "<?php\n/**\n * @psalm-param array<int|string, list<int>> $x\n * @phpstan-return array{0: int, 1?: string}\n */\nfunction f21($x) { return [1]; }\n", &[]),
        ("8.5", "<?php\n/**\n * @template T of \\Countable\n * @template-extends \\ArrayObject<T>\n * @mixin \\stdClass\n */\nclass A20 extends \\ArrayObject {}\n", &[]),
        ("8.5", "<?php\n/**\n * @method static int foo(string $a, int ...$b)\n * @method void bar()\n * @property-read int $p\n * @property int|null $q Some text\n */\nclass A18 {}\n", &[]),
        ("8.5", "<?php\n/**\n * @method foo(\n */\nclass A19 {}\n", &["P1129 27..29 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/**\n * @ORM\\Column(type=\"string\", length=255)\n * @Assert\\NotBlank()\n */\nfunction f16() {}\n", &[]),
        ("8.5", "<?php\n/**\n * @param array< int, string > $x\n */\nfunction f11($x) {}\n", &[]),
        ("8.5", "<?php\n/**\n * @param int $x {\n */\nfunction f9($x) {}\n", &[]),
        ("8.5", "<?php\n/**\n * @param string $x <b>bold</b>\n * @return string|false <p>Some doc</p>\n */\nfunction f22($x) { return ''; }\n", &[]),
        ("8.5", "<?php\n/**\n * {@inheritdoc}\n * @param int $x {@link http://x}\n */\nfunction f17($x) {}\n", &[]),
        ("8.5", "<?php\n/**\n * @Param int $x\n * @PARAM string $y\n */\nfunction f15($x, $y) {}\n", &[]),
        ("8.5", "<?php\n/** @param int */\nfunction f1($x) {}\n", &["P1129 21..23 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/**\n * @param int\n */\nfunction f2($x) {}\n", &["P1129 25..27 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/**\n * @param int The value\n */\nfunction f3($x) {}\n", &[]),
        ("8.5", "<?php\n/**\n * @param array<int, string>\n * @return void\n */\nfunction f4($x) {}\n", &["P1129 42..49 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/**\n * @param ?int[] $x\n * @param int[]|null $y\n * @param (int|string)[] $z\n */\nfunction f12($x, $y, $z) {}\n", &[]),
        ("8.5", "<?php\n/**\n * @param int &$x\n * @param string ...$y\n * @param int &...$z\n */\nfunction f5(&$x, ...$y) {}\n", &[]),
        ("8.5", "<?php\n/**\n * @param A14::* $x\n * @param A14::F* $y\n */\nfunction f14($x, $y) {}\nclass A14 { const FA = 1; }\n", &[]),
        ("8.5", "<?php\nclass A13 {\n /** @return static|$this|self */\n function f() { return $this; }\n}\n", &[]),
        ("8.5", "<?php\n/**\n * @param int | string $x\n */\nfunction f10($x) {}\n", &[]),
        ("8.5", "<?php\n/**\n * @return int Description (x\n */\nfunction f6() { return 1; }\n", &[]),
        ("8.5", "<?php\n/** @return */\nfunction f7() {}\n", &["P1129 18..20 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/**\n * @throws\n */\nfunction f8() {}\n", &["P1129 22..24 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/** @var */\n$x = 1;\n", &["P1129 15..17 PHPDoc parse error. Unexpected token."]),
        ("8.5", "<?php\n/** @var int $x Some text */\n$x = 1;\n", &[]),
        ("8.5", "<?php\ntrait Qt { public const X = 1; }\nclass Qc { use Qt; public const X = 2; }\n", &["P1111 71..76 In the composition of type 'Qc', class constants 'Qc::X' and 'Qt::X' are not compatible."]),
        ("8.5", "<?php\ntrait Mt2 { abstract public function m(int $a): void; }\nclass Mc2 { use Mt2; public function m(): void {} }\n", &["P1038 83..108 Method 'Mc2::m()' is not compatible with method 'Mt2::m()'."]),
        ("8.5", "<?php\nclass Pa { public int $p = 0; }\nclass Pb extends Pa { public string $p = ''; }\n", &["P1077 60..82 Property Pb::$p must have type int."]),
        ("8.5", "<?php\ninterface Pi { public int $p { get; } }\nclass Pc implements Pi { public string $p = ''; }\n", &["P1077 71..93 Property Pc::$p must have type int or narrower"]),
        ("8.5", "<?php\ninterface Pk { public int|string $p { get; } }\nclass Pe implements Pk { public int $p = 0; }\n", &[]),
        ("8.5", "<?php\ninterface Pj { public int $p { set; } }\nclass Pd implements Pj { public bool $p = false; }\n", &["P1077 71..94 Property Pd::$p must have type int or wider"]),
        ("8.5", "<?php\ninterface Vi1 { public function m(); }\nclass Vc1 implements Vi1 { protected function m() {} }\n", &["P1038 72..94 Method 'Vc1::m()' is not compatible with method 'Vi1::m()'."]),
        ("8.5", "<?php\nclass Va1 { public function m() {} }\nclass Vb1 extends Va1 { protected function m() {} }\n", &["P1038 67..89 Method 'Vb1::m()' is not compatible with method 'Va1::m()'."]),
        ("8.5", "<?php\nclass Va2 { public $p; }\nclass Vb2 extends Va2 { private $p; }\n", &["P1070 63..65 Visibility of 'Vb2::$p' must be the same or less restrictive than 'Va2::$p'."]),
    ];
    let mut failures = Vec::new();
    let mut workspaces: std::collections::HashMap<&str, Workspace> = std::collections::HashMap::new();
    for (version, text, expected) in cases {
        let ws = workspaces.entry(version).or_insert_with(|| {
            let mut ws =
                Workspace::new(Config { php_version: version.parse().unwrap(), ..Default::default() });
            ws.load_stubs();
            ws
        });
        let id = ws.set_file("file:///c.php", text.to_string());
        let mut got: Vec<String> = ws
            .diagnostics(id)
            .into_iter()
            .filter(|d| CODES.contains(&d.code.as_str()))
            .map(|d| {
                format!(
                    "{} {}..{} {}",
                    d.code.as_str(),
                    u32::from(d.range.start()),
                    u32::from(d.range.end()),
                    d.message
                )
            })
            .collect();
        got.sort();
        if got != *expected {
            failures.push(format!("{version} {text:?}\n  expected {expected:?}\n  got      {got:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Completion gaps found against Intelephense (fourth comparison).
#[test]
fn completion_parity() {
    let generics = r#"<?php
/**
 * @template TKey
 * @template TValue
 */
trait Enumerates {
    /** @param callable(TValue, TKey): mixed $cb
     *  @return $this */
    public function each(callable $cb) { return $this; }
}
/**
 * @template TKey
 * @template TValue
 */
class Coll {
    /** @use Enumerates<TKey, TValue> */
    use Enumerates;
    /** @param iterable<TKey, TValue> $items */
    public function __construct($items = []) {}
    /** @param callable(TValue, TKey): bool $cb
     *  @return static */
    public function filter(callable $cb) { return $this; }
    /** @param callable($this): mixed $cb */
    public function pipe(callable $cb) {}
}
class Event { public function finish() {} }
class Maker { public function make() { return new Event(); } }
/** @return Event[] */
function events() { return []; }
"#;
    let types = |body: &str| type_at(&format!("{generics}{body}"));
    assert_eq!(types("(new Coll(events()))->filter(fn ($v) => $v<|>);"), "Event");
    assert_eq!(
        types("(new Coll(events()))->filter(fn ($v) => true)->each(function ($e) { $e<|>; });"),
        "Event"
    );
    assert!(types("(new Coll(events()))->pipe(fn ($c) => $c<|>);").starts_with("Coll<"));
    // return types inferred from the body
    assert_eq!(types("$e = (new Maker())->make(); $e<|>;"), "Event");

    let members = r#"<?php
trait Macroable { public function __call($m, $a) {} private static function helper() {} }
class Uses { use Macroable { __call as macroCall; } public function f() { self::<|> } }
"#;
    let l = completion_labels(members);
    assert!(l.contains(&"macroCall".to_string()) && l.contains(&"helper".to_string()), "{l:?}");
    let l = completion_labels("<?php\nenum Suit: string { case H = 'h'; }\nSuit::<|>");
    for want in ["H", "cases", "from", "tryFrom", "class"] {
        assert!(l.contains(&want.to_string()), "{want} {l:?}");
    }
    // `@see Foo::` in doc comments
    let l = completion_labels(
        "<?php\nclass Foo { public $bar; public function baz() {} }\n/** @see Foo::<|> */\nfunction f() {}",
    );
    assert!(l.contains(&"baz".to_string()) && l.contains(&"$bar".to_string()), "{l:?}");
    // anonymous classes inherit the parameter types of the interface
    let t = type_at("<?php\nclass Model {}\ninterface Cast { /** @param Model $m */ public function get($m); }\n$c = new class implements Cast { public function get($m) { $m<|>; } };");
    assert_eq!(t, "Model");
}

/// Go to definition like Intelephense (fourth comparison).
#[test]
fn definition_parity() {
    // `$this` and `Foo::class` have no target
    assert!(goto("<?php\nclass A { function f() { $th<|>is; } }").is_empty());
    assert!(goto("<?php\nclass A {}\n$x = A::cl<|>ass;").is_empty());
    // the first definition in the scope
    let f = fixture("<?php\nfunction f($id) {\n    $id = (string) $id;\n    return $i<|>d;\n}");
    let g = f.ws.goto_definition(f.file, f.offset);
    assert_eq!(g.len(), 1);
    assert_eq!(u32::from(g[0].focus.start()), 17, "the parameter");
    // doc comments: @param of a promoted parameter, @see, @method
    let g = goto("<?php\nclass E {\n    /** @param string $gu<|>ard */\n    public function __construct(public string $guard) {}\n}");
    assert_eq!(g.len(), 1, "{g:?}");
    let g = goto("<?php\nclass R { public function register() {} public $items; }\n/** @see R::regi<|>ster() */\nfunction f() {}");
    assert_eq!(g.len(), 1, "{g:?}");
    let g = goto("<?php\nclass R { public $items; }\n/** @see R::$ite<|>ms */\nfunction f() {}");
    assert_eq!(g.len(), 1, "{g:?}");
    let g = goto("<?php\nfunction helper() {}\n/** @see help<|>er() */\nfunction f() {}");
    assert_eq!(g.len(), 1, "{g:?}");
    let g = goto("<?php\n/** @method static int conf<|>igure() */\nclass F {}");
    assert_eq!(g.len(), 1, "{g:?}");
}

/// References as Intelephense reports them.
#[test]
fn references_parity() {
    let refs = |text: &str| -> Vec<String> {
        let f = fixture(text);
        f.ws.references(f.file, f.offset, true)
            .iter()
            .map(|r| {
                let file = f.ws.file(r.file).unwrap();
                let line = file.text[..usize::from(r.range.start())].matches('\n').count();
                format!("{}:{}", line, &file.text[r.range])
            })
            .collect()
    };
    // a method together with the methods it overrides / that override it
    let text = "<?php\ninterface I { public function run(); }\nclass A implements I { public function run() {} }\nclass B extends A { public function run() {} }\nfunction f(A $a) { $a->ru<|>n(); }\n";
    assert_eq!(refs(text), vec!["1:run", "2:run", "3:run", "4:run"]);
    // `new` calls the constructor
    let text = "<?php\nclass A { public function __con<|>struct() {} }\n$a = new A();\n";
    assert_eq!(refs(text), vec!["1:__construct", "2:A"]);
    // `$this` in its method only
    let text =
        "<?php\nclass A {\n function f() { $this->g(); $t<|>his->h(); }\n function g() { $this; }\n}\n";
    assert_eq!(refs(text), vec!["2:$this", "2:$this"]);
    // parameters: `@param` and the same parameter of related methods
    let text = "<?php\nclass A {\n /** @param int $x */\n function f($x) { return $x<|>; }\n}\nclass B extends A {\n function f($x) { return $x; }\n}\n";
    assert_eq!(refs(text), vec!["2:$x", "3:$x", "3:$x", "6:$x", "6:$x"]);
    // trait methods of implementing classes, overridden properties
    let text = "<?php\ninterface I { function all(); }\ntrait T { function all() {} }\nclass C implements I { use T; }\nfunction f(C $c) { $c->al<|>l(); }\n";
    assert_eq!(refs(text), vec!["1:all", "2:all", "4:all"]);
    let text = "<?php\nclass A { protected $ops = []; }\nclass B extends A { protected $o<|>ps = [1]; }\n";
    assert_eq!(refs(text), vec!["1:$ops", "2:$ops"]);
    // classes in `@method` parameter types
    let text = "<?php\nclass Fi<|>le {}\n/** @method void open(File $f, int $n) */\nclass B {}\n";
    assert_eq!(refs(text), vec!["1:File", "2:File"]);
    // `@see` tags
    let text = "<?php\nclass Ut<|>il {}\n/** {@see Util::x()} and @see Util */\nclass B {}\n";
    assert_eq!(refs(text), vec!["1:Util", "2:Util", "2:Util"]);
    // global variables
    let text = "<?php\n$db = 1;\nfunction f() { global $d<|>b; }\nfunction g() { global $db; return $db; }\nfunction h() { $db = 2; }\n";
    assert_eq!(refs(text), vec!["1:$db", "2:$db", "3:$db", "3:$db"]);
    // qualified names are reported whole, once
    let text = "<?php\nnamespace App;\nclass Fo<|>o {}\n$a = new \\App\\Foo();\n";
    assert_eq!(refs(text), vec!["2:Foo", "3:\\App\\Foo"]);
    let text = "<?php\nnamespace Foo;\nclass Fo<|>o {}\n$a = new \\Foo\\Foo();\n";
    assert_eq!(refs(text), vec!["2:Foo", "3:\\Foo\\Foo"]);
    // like Intelephense: string arguments naming a global function and
    // `@see f()`, not other strings
    let text = "<?php\n/** @see myfn() @see myfn */\nfunction my<|>fn() {}\nfunction_exists('myfn');\nadd_filter('x', \"MYFN\");\n$a = 'myfn';\nfoo('myfn' . '');\n";
    assert_eq!(refs(text), vec!["1:myfn", "2:myfn", "3:myfn", "4:MYFN"]);
    let text = "<?php\nnamespace App;\nfunction my<|>fn() {}\ncall_user_func('App\\myfn');\n";
    assert_eq!(refs(text), vec!["2:myfn"]);
    // promoted constructor parameters: the property, the parameter and
    // `compact()` arguments
    let text = "<?php\nclass E {\n public function __construct(private string $da<|>ta) {\n  echo $data;\n  compact('data');\n }\n function f() { return $this->data; }\n}\n";
    assert_eq!(refs(text), vec!["2:$data", "3:$data", "4:data", "6:data"]);
    let text = "<?php\nfunction h($da<|>ta) { return compact('data', 'x'); }\n";
    assert_eq!(refs(text), vec!["1:$data", "1:data"]);
    // global variables typed by `new` at file level in another file
    let text = "//- a.php\n<?php\nclass A { function fo<|>o() {} }\nfunction g() { global $x; $x->foo(); }\n//- b.php\n<?php\n$x = new A();\n";
    assert_eq!(refs(text), vec!["1:foo", "2:foo"]);
}
