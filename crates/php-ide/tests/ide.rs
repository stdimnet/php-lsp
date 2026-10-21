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
        "P1005 Expected 2 arguments. Found 1.",
        "P1005 Expected 2 arguments. Found 3.",
        "P1013 Undefined method 'missing'.",
        "P1014 Undefined property '$nope'.",
        "P1012 Undefined class constant 'NOPE'.",
        "P1016 Cannot instantiate interface 'App\\Shape'.",
        "P1009 Undefined type 'App\\Nope'.",
        "P1010 Call to unknown function: 'undefined_fn'",
        "P1008 Undefined variable '$undefinedVar'.",
        "P1003 Symbol '$unused' is declared but not used.",
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
    assert!(d.is_empty(), "{d:#?}");
}
