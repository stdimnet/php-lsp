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
    assert!(ws.format(id, None, &Default::default()).unwrap().is_empty());
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
