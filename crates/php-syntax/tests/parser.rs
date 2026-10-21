use expect_test::{expect, Expect};
use php_syntax::{parse, PhpVersion};

fn check(text: &str, expect: Expect) {
    let p = parse(text, PhpVersion::LATEST);
    assert_eq!(p.syntax_node().text().to_string(), text);
    expect.assert_eq(&p.debug_tree());
}

fn ok(text: &str) {
    let p = parse(text, PhpVersion::LATEST);
    assert_eq!(p.syntax_node().text().to_string(), text);
    assert!(p.errors().is_empty(), "unexpected errors for {text:?}:\n{}", p.debug_tree());
}

fn errors(text: &str, version: PhpVersion) -> Vec<String> {
    let p = parse(text, version);
    assert_eq!(p.syntax_node().text().to_string(), text);
    p.errors().iter().map(|e| e.message.clone()).collect()
}

#[test]
fn valid_snippets() {
    let snippets = [
        "<?php namespace A\\B; use C\\D, E\\F as G; use function h\\i; use const J\\K;",
        "<?php use A\\{B, C as D, function e, const F,};",
        "<?php namespace { echo 1; } namespace Foo { echo 2; }",
        "<?php declare(strict_types=1);",
        "<?php #[A(1), B] final readonly class X extends Y implements Z, W { use T1, T2 { T1::f insteadof T2; f as protected g; } }",
        "<?php abstract class A { abstract public function f(int|string $a, ?B &...$rest): static; }",
        "<?php interface I extends J, K { const X = 1; public function f(); }",
        "<?php trait T { public static int $x = 1; private ?array $y; }",
        "<?php enum Suit: string implements HasLabel { case Hearts = 'H'; case Spades = 'S'; const Wild = self::Spades; public function label(): string { return ucfirst($this->name); } }",
        "<?php class P { public function __construct(private readonly int $x = 0, protected A&B $y) {} }",
        "<?php class H { public string $name { get => strtoupper($this->name); set(string $v) { $this->name = $v; } } public private(set) int $id; }",
        "<?php class C { const int|string A = 1; final public const B = 2; }",
        "<?php function f((A&B)|null $x): never { throw new E(); }",
        "<?php $f = fn($x) => $x * 2; $g = static function () use (&$a, $b): int { return 1; };",
        "<?php $x = match(true) { $a > 1, $a < 0 => 'a', default => 'b', };",
        "<?php $a ??= $b ?? $c ?: $d ? 1 : 2;",
        "<?php [$a, [, $b]] = $c; list('x' => $y) = $z; [...$a, ...$b];",
        "<?php $a = new class(1) extends B { public function f() {} };",
        "<?php new Foo; new Foo(); new $cls; new static; new (trim($x)); new static::$cls[0]($y);",
        "<?php $a?->b?->c(); $a::$b; A::B; A::class; $a::class; A::{'b'}; $a->{$b}(); $$a; ${'a'};",
        "<?php $s = \"a $b {$c->d} ${e} $f[0] $g->h {$i['j']}\";",
        "<?php $h = <<<EOT\n  hello $name\n  {$x->y}\n  EOT;\n$n = <<<'EOT'\nraw $x\nEOT;\n",
        "<?php $r = `ls $dir`;",
        "<?php if ($a): echo 1; elseif ($b): echo 2; else: echo 3; endif;",
        "<?php foreach ($a as $k => &$v): endforeach; while (1): endwhile; for (;;): endfor; switch ($a): case 1: break; endswitch;",
        "<?php switch ($a) { case 1; case 2: echo 1; break; default: return; }",
        "<?php try { f(); } catch (A|B $e) { } catch (C) { } finally { }",
        "<?php do { $i++; } while ($i < 10);",
        "<?php for ($i = 0, $j = 1; $i < 10; $i++, $j--) {}",
        "<?php static $a = 1, $b; global $c, $d; unset($a, $b,);",
        "<?php a: goto a;",
        "<?php echo 1, 2; print 3; exit; die(1); exit();",
        "<?php isset($a, $b['c']); empty($a); eval('1;'); include 'a.php'; require_once __DIR__ . '/b.php';",
        "<?php $a = (int) $b + (string)$c . (bool) $d;",
        "<?php $a = -$b ** 2; $c = !$d instanceof E; $f = @g(); $h = ~$i; $j = clone $k;",
        "<?php $a = yield; $b = yield $c; $d = yield $e => $f; yield from g();",
        "<?php f(...$args); f(name: 1, array: 2); $f = strlen(...); $o->m(...);",
        "<?php $a = [1, 'b' => 2, &$c, ...$d,];",
        "<?php $x = array(1, 2, array(3));",
        "<html><?php if ($a) { ?><b><?= $x ?></b><?php } ?></html>",
        "<?php echo 1 ?>",
        "<?php function &refFn(): array { static $cache = []; return $cache; }",
        "<?php function readonly() {} enum E {} $enum = 1; enum($x);",
        "<?php $a = $b and $c or $d xor $e;",
        "<?php $a = 1_000_000 + 0x1F + 0b101 + 0o17 + 017 + 1.5e-3 + .5;",
        "<?php __halt_compiler(); raw data ?> <?php",
        "<?php $a = new Foo()->bar(); $b = new Foo()::BAR; new Foo()();",
        "<?php #[Attr] function f(#[SensitiveParameter] $x) {} $f = #[A] fn() => 1; $g = #[A] static function() {};",
        "<?php class A { public function list() {} public function new() {} const FOREACH = 1; } A::list(); $a->new(); A::FOREACH;",
        "<?php $a = $b->c[0]->d['e']()()->f;",
        "<?php $x = !$a = foo();",
        "<?php $closure = function() { return $this; }; $c = (function() {})();",
        "<?php echo \"{$a}\" . \"$a\" . '$a' . \"\\$a\";",
        "<?php // comment ?>\n<p>html</p>",
        "<?php /** doc */ function f() {} # hash comment\n",
    ];
    for s in snippets {
        ok(s);
    }
}

#[test]
fn version_gating() {
    assert!(errors("<?php enum A {}", PhpVersion::PHP_8_0).iter().any(|e| e.contains("enums")));
    assert!(!errors("<?php $x = match($a) { default => 1 };", PhpVersion::PHP_7_4).is_empty());
    assert!(errors("<?php f(a: 1);", PhpVersion::PHP_7_4).iter().any(|e| e.contains("named arguments")));
    assert!(errors("<?php $a ??= 1;", PhpVersion::PHP_7_3).iter().any(|e| e.contains("??=")));
    assert!(errors("<?php fn() => 1;", PhpVersion::PHP_7_3).iter().any(|e| e.contains("arrow functions")));
    assert!(errors("<?php function f(int|string $a) {}", PhpVersion::PHP_7_4)
        .iter()
        .any(|e| e.contains("union types")));
    assert!(errors("<?php readonly class A {}", PhpVersion::PHP_8_1)
        .iter()
        .any(|e| e.contains("readonly classes")));
    assert!(!errors("<?php $a = $b?->c;", PhpVersion::PHP_7_4).is_empty());
    assert!(errors("<?php $a = $b{0};", PhpVersion::PHP_7_4).is_empty());
    assert!(errors("<?php enum A {}", PhpVersion::PHP_8_1).is_empty());
}

#[test]
fn recovery_keeps_going() {
    let text =
        "<?php\nfunction f( {\n  $a = ;\n}\nclass A { public function g() { return 1 } }\nfunction h() {}\n";
    let p = parse(text, PhpVersion::LATEST);
    assert_eq!(p.syntax_node().text().to_string(), text);
    assert!(!p.errors().is_empty());
    let tree = p.debug_tree();
    assert!(tree.contains("CLASS_DECL"), "{tree}");
    assert_eq!(tree.matches("FUNCTION_DECL").count(), 2, "{tree}");
}

#[test]
fn incomplete_member_access() {
    let p = parse("<?php $a->", PhpVersion::LATEST);
    assert!(p.debug_tree().contains("PROPERTY_FETCH_EXPR"));
    let p = parse("<?php Foo::", PhpVersion::LATEST);
    assert!(p.debug_tree().contains("CLASS_CONST_FETCH_EXPR"));
}

#[test]
fn tree_shape_method_call() {
    check(
        "<?php $a->b(1);",
        expect![[r#"
            SOURCE_FILE@0..15
              OPEN_TAG@0..6 "<?php "
              EXPR_STMT@6..15
                METHOD_CALL_EXPR@6..14
                  VARIABLE_EXPR@6..8
                    VARIABLE@6..8 "$a"
                  ARROW@8..10 "->"
                  MEMBER_NAME@10..11
                    IDENT@10..11 "b"
                  ARG_LIST@11..14
                    L_PAREN@11..12 "("
                    ARG@12..13
                      LITERAL@12..13
                        INT_NUMBER@12..13 "1"
                    R_PAREN@13..14 ")"
                SEMICOLON@14..15 ";"
        "#]],
    );
}

#[test]
fn tree_shape_precedence() {
    check(
        "<?php $a = 1 + 2 * 3;",
        expect![[r#"
            SOURCE_FILE@0..21
              OPEN_TAG@0..6 "<?php "
              EXPR_STMT@6..21
                ASSIGN_EXPR@6..20
                  VARIABLE_EXPR@6..8
                    VARIABLE@6..8 "$a"
                  WHITESPACE@8..9 " "
                  EQ@9..10 "="
                  WHITESPACE@10..11 " "
                  BIN_EXPR@11..20
                    LITERAL@11..12
                      INT_NUMBER@11..12 "1"
                    WHITESPACE@12..13 " "
                    PLUS@13..14 "+"
                    WHITESPACE@14..15 " "
                    BIN_EXPR@15..20
                      LITERAL@15..16
                        INT_NUMBER@15..16 "2"
                      WHITESPACE@16..17 " "
                      STAR@17..18 "*"
                      WHITESPACE@18..19 " "
                      LITERAL@19..20
                        INT_NUMBER@19..20 "3"
                SEMICOLON@20..21 ";"
        "#]],
    );
}

#[test]
fn doc_comment_precedes_decl() {
    use php_syntax::ast::{self, AstNode};
    let p = parse("<?php\n/** Hello */\nfunction f() {}", PhpVersion::LATEST);
    let f = p.syntax_node().descendants().find_map(ast::FunctionDecl::cast).unwrap();
    assert_eq!(ast::doc_comment(f.syntax()).unwrap().text(), "/** Hello */");
    assert_eq!(f.name().unwrap().text(), "f");
}
