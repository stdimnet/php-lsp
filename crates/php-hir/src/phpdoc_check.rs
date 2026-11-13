//! Syntax check of the types in PHPDoc tags (Intelephense P1129 "PHPDoc
//! parse error. Unexpected token.").
//!
//! Like Intelephense, the comment is read as one token stream: a type may
//! continue on the next line, and a missing type or variable is reported at
//! the following tag or at `*/`. Descriptions are free text.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `*/`
    End,
    /// `@param` (first on its line)
    Tag,
    Var,
    Name,
    Str,
    Num,
    Punct,
}

#[derive(Debug, Clone, Copy)]
struct Tok {
    kind: Kind,
    start: usize,
    end: usize,
    /// No whitespace before the token.
    adjacent: bool,
    /// First token of its line (after the leading `*`).
    line_start: bool,
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\\' || c == '-' || !c.is_ascii()
}

fn lex(text: &str) -> Vec<Tok> {
    let bytes = text.as_bytes();
    let mut out: Vec<Tok> = Vec::new();
    let mut i = if text.starts_with("/**") { 3 } else { 0 };
    // the text right after `/**` counts as the start of a line
    let mut line_start = true;
    let mut last_end = i;
    while i < bytes.len() {
        let c = text[i..].chars().next().unwrap_or(' ');
        if c.is_whitespace() {
            if c == '\n' {
                line_start = true;
            }
            i += c.len_utf8();
            continue;
        }
        // `// comment` inside a type (PHPStan shapes)
        if text[i..].starts_with("//") {
            i += text[i..].find('\n').unwrap_or(text.len() - i);
            continue;
        }
        let adjacent = i == last_end;
        let start = i;
        let kind = if text[i..].starts_with("*/") {
            i += 2;
            Kind::End
        } else if c == '*' && line_start {
            // the leading `*` of a line
            i += 1;
            last_end = i;
            continue;
        } else if c == '@' && text[i + 1..].starts_with(|n: char| n.is_ascii_alphabetic()) {
            i += 1;
            while let Some(n) = text[i..].chars().next().filter(|n| is_name_char(*n) || *n == ':') {
                i += n.len_utf8();
            }
            if line_start {
                Kind::Tag
            } else {
                Kind::Name
            }
        } else if c == '$' {
            i += 1;
            while let Some(n) =
                text[i..].chars().next().filter(|n| n.is_alphanumeric() || *n == '_' || !n.is_ascii())
            {
                i += n.len_utf8();
            }
            Kind::Var
        } else if c == '\'' || c == '"' {
            i += 1;
            while i < bytes.len() && bytes[i] != c as u8 && bytes[i] != b'\n' {
                i += if bytes[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(bytes.len());
            Kind::Str
        } else if c.is_ascii_digit() || c == '-' && text[i + 1..].starts_with(|n: char| n.is_ascii_digit()) {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.' || bytes[i] == b'_')
            {
                i += 1;
            }
            Kind::Num
        } else if is_name_char(c) && c != '-' {
            // `Foo::BAR_*`: `*` belongs to names after `::`
            let after_colons = out.last().is_some_and(|t| &text[t.start..t.end] == "::");
            // wildcards: `Foo::BAR_*`, `\SIG*`
            while let Some(n) = text[i..].chars().next().filter(|n| {
                is_name_char(*n) || *n == '*' && !text[i..].starts_with("*/") && (after_colons || i > start)
            }) {
                i += n.len_utf8();
            }
            Kind::Name
        } else if text[i..].starts_with("...") {
            i += 3;
            Kind::Punct
        } else if text[i..].starts_with("::") || text[i..].starts_with("()") {
            // `()` is one token, like in Intelephense's lexer
            i += 2;
            Kind::Punct
        } else {
            i += c.len_utf8();
            Kind::Punct
        };
        while !text.is_char_boundary(i.min(text.len())) {
            i += 1;
        }
        i = i.min(text.len());
        out.push(Tok { kind, start, end: i, adjacent, line_start });
        line_start = false;
        last_end = i;
    }
    out
}

struct Parser<'a> {
    text: &'a str,
    toks: Vec<Tok>,
    pos: usize,
}

type Res = Result<(), usize>;

const CALLABLES: &[&str] = &["callable", "closure", "\\closure", "pure-callable", "pure-closure"];

impl Parser<'_> {
    fn tok(&self, i: usize) -> Option<&Tok> {
        self.toks.get(i)
    }

    fn text_of(&self, i: usize) -> &str {
        self.tok(i).map_or("", |t| &self.text[t.start..t.end])
    }

    fn is(&self, s: &str) -> bool {
        self.tok(self.pos).is_some_and(|t| t.kind == Kind::Punct || t.kind == Kind::Name)
            && self.text_of(self.pos) == s
    }

    fn adjacent(&self, s: &str) -> bool {
        self.is(s) && self.tok(self.pos).is_some_and(|t| t.adjacent)
    }

    /// The end of the tag's content: `*/` or the next tag.
    fn at_end(&self) -> bool {
        self.tok(self.pos).is_none_or(|t| matches!(t.kind, Kind::End | Kind::Tag))
    }

    fn expect(&mut self, s: &str) -> Res {
        if self.is(s) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.pos)
        }
    }

    fn ty(&mut self) -> Res {
        self.intersection()?;
        while self.is("|") {
            self.pos += 1;
            self.intersection()?;
        }
        Ok(())
    }

    fn intersection(&mut self) -> Res {
        self.postfix()?;
        // `int &$x` is a by-reference parameter
        while self.is("&")
            && !matches!(self.tok(self.pos + 1).map(|t| t.kind), Some(Kind::Var))
            && self.text_of(self.pos + 1) != "..."
        {
            self.pos += 1;
            self.postfix()?;
        }
        Ok(())
    }

    fn postfix(&mut self) -> Res {
        self.atom()?;
        while self.adjacent("[") {
            self.pos += 1;
            // `T['key']`: offset access
            if !self.is("]") {
                self.ty()?;
            }
            self.expect("]")?;
        }
        Ok(())
    }

    fn atom(&mut self) -> Res {
        let Some(t) = self.tok(self.pos).copied() else { return Err(self.pos) };
        match t.kind {
            Kind::Var | Kind::Str | Kind::Num => {
                self.pos += 1;
                Ok(())
            }
            Kind::Punct if self.is("?") => {
                self.pos += 1;
                self.postfix()
            }
            Kind::Punct if self.is("(") => {
                self.pos += 1;
                self.ty()?;
                // `($x is int ? A : B)`
                if self.is("is") {
                    self.pos += 1;
                    if self.is("not") {
                        self.pos += 1;
                    }
                    self.ty()?;
                    self.expect("?")?;
                    self.ty()?;
                    self.expect(":")?;
                    self.ty()?;
                }
                self.expect(")")
            }
            Kind::Name => {
                let name = self.text_of(self.pos).to_ascii_lowercase();
                self.pos += 1;
                if self.adjacent("::") {
                    self.pos += 1;
                    return match self.tok(self.pos).map(|t| t.kind) {
                        Some(Kind::Name) => {
                            self.pos += 1;
                            Ok(())
                        }
                        _ if self.is("*") => {
                            self.pos += 1;
                            Ok(())
                        }
                        _ => Err(self.pos),
                    };
                }
                if self.adjacent("<") {
                    self.pos += 1;
                    self.generic_args()?;
                }
                if self.adjacent("{") {
                    self.pos += 1;
                    self.shape()?;
                }
                if CALLABLES.contains(&name.as_str()) && (self.is("(") || self.is("()")) {
                    // `callable (int $a)`: Intelephense does not allow the space
                    if !self.tok(self.pos).is_some_and(|t| t.adjacent) {
                        return Err(self.pos);
                    }
                    if self.is("()") {
                        self.pos += 1;
                    } else {
                        self.pos += 1;
                        self.callable_params()?;
                    }
                    if self.is(":") {
                        self.pos += 1;
                        self.postfix()?;
                    }
                }
                Ok(())
            }
            _ => Err(self.pos),
        }
    }

    fn generic_args(&mut self) -> Res {
        loop {
            if self.is(">") {
                self.pos += 1;
                return Ok(());
            }
            if (self.is("covariant") || self.is("contravariant"))
                && self.tok(self.pos + 1).is_some_and(|t| !matches!(t.kind, Kind::Punct))
            {
                self.pos += 1;
            }
            if self.is("*") {
                self.pos += 1;
            } else {
                self.ty()?;
            }
            if self.is(",") {
                self.pos += 1;
            } else if !self.is(">") {
                return Err(self.pos);
            }
        }
    }

    fn shape(&mut self) -> Res {
        loop {
            if self.is("}") {
                self.pos += 1;
                return Ok(());
            }
            if self.is("...") {
                self.pos += 1;
                if self.adjacent("<") {
                    self.pos += 1;
                    self.generic_args()?;
                }
            } else {
                let key =
                    matches!(self.tok(self.pos).map(|t| t.kind), Some(Kind::Name | Kind::Str | Kind::Num));
                let next = self.text_of(self.pos + 1);
                if key && (next == ":" || next == "?" && self.text_of(self.pos + 2) == ":") {
                    self.pos += if next == "?" { 3 } else { 2 };
                }
                self.ty()?;
            }
            if self.is(",") {
                self.pos += 1;
            } else if !self.is("}") {
                return Err(self.pos);
            }
        }
    }

    fn callable_params(&mut self) -> Res {
        loop {
            if self.is(")") {
                self.pos += 1;
                return Ok(());
            }
            // `...mixed`, `&int`
            for p in ["&", "..."] {
                if self.is(p) {
                    self.pos += 1;
                }
            }
            self.ty()?;
            for p in ["&", "..."] {
                if self.is(p) {
                    self.pos += 1;
                }
            }
            if self.tok(self.pos).is_some_and(|t| t.kind == Kind::Var) {
                self.pos += 1;
            }
            if self.is("=") {
                self.pos += 1;
                // a default value
                if self.tok(self.pos).is_some_and(|t| matches!(t.kind, Kind::Name | Kind::Num | Kind::Str)) {
                    self.pos += 1;
                }
            }
            if self.is(",") {
                self.pos += 1;
            } else if !self.is(")") {
                return Err(self.pos);
            }
        }
    }

    /// `$x`, `&$x`, `...$x`, `&...$x`
    fn at_variable(&self) -> bool {
        let mut i = self.pos;
        for p in ["&", "..."] {
            if self.text_of(i) == p {
                i += 1;
            }
        }
        self.tok(i).is_some_and(|t| t.kind == Kind::Var)
    }

    /// The content of the tag starting at token `self.pos`.
    fn tag(&mut self, name: &str) -> Res {
        match name {
            "param" | "property" | "property-read" | "property-write" => {
                if self.at_variable() {
                    return Ok(());
                }
                if self.at_end() {
                    return Err(self.pos);
                }
                self.ty()?;
                if self.at_end() {
                    return Err(self.pos);
                }
                Ok(())
            }
            "return" | "var" | "throws" => {
                if self.at_end() {
                    return Err(self.pos);
                }
                self.ty()
            }
            "template" | "template-covariant" | "template-contravariant" => {
                if self.at_end() {
                    return Err(self.pos);
                }
                self.pos += 1;
                if self.is("of") || self.is("as") || self.is("super") {
                    self.pos += 1;
                    self.ty()?;
                }
                Ok(())
            }
            "method" => {
                if self.at_end() {
                    return Err(self.pos);
                }
                // `static` is a modifier unless the name follows (`static foo()` returns static)
                let static_return = matches!(self.text_of(self.pos + 2), "(" | "()");
                if self.is("static") && self.text_of(self.pos + 1) != "(" && !static_return {
                    self.pos += 1;
                }
                // a return type unless the name and `(` follow directly
                let direct = self.tok(self.pos).is_some_and(|t| t.kind == Kind::Name)
                    && self.text_of(self.pos + 1) == "("
                    && self.tok(self.pos + 1).is_some_and(|t| t.adjacent);
                if !direct {
                    self.ty()?;
                    if !self.tok(self.pos).is_some_and(|t| t.kind == Kind::Name) {
                        return Err(self.pos);
                    }
                }
                self.pos += 1;
                if self.is("()") {
                    self.pos += 1;
                    return Ok(());
                }
                // without parentheses (Intelephense's handling of these is not consistent)
                if !self.is("(") {
                    return Ok(());
                }
                // balanced parameter list
                let mut depth = 0;
                while let Some(t) = self.tok(self.pos).copied() {
                    if matches!(t.kind, Kind::End | Kind::Tag) {
                        return Err(self.pos);
                    }
                    match self.text_of(self.pos) {
                        "(" => depth += 1,
                        ")" => {
                            depth -= 1;
                            if depth == 0 {
                                self.pos += 1;
                                return Ok(());
                            }
                        }
                        _ => {}
                    }
                    self.pos += 1;
                }
                Err(self.pos)
            }
            _ => Ok(()),
        }
    }
}

/// Byte ranges (relative to `comment`) of the unexpected tokens.
pub fn syntax_errors(comment: &str) -> Vec<(usize, usize)> {
    let toks = lex(comment);
    let mut p = Parser { text: comment, toks, pos: 0 };
    let mut out = Vec::new();
    let mut i = 0;
    while i < p.toks.len() {
        let t = p.toks[i];
        if t.kind != Kind::Tag || !t.line_start {
            i += 1;
            continue;
        }
        let raw = comment[t.start + 1..t.end].to_ascii_lowercase();
        let name =
            raw.strip_prefix("phpstan-").or_else(|| raw.strip_prefix("psalm-")).unwrap_or(&raw).to_string();
        p.pos = i + 1;
        if let Err(at) = p.tag(&name) {
            let range = p.toks.get(at).map_or((comment.len(), comment.len()), |t| (t.start, t.end));
            out.push(range);
        }
        // continue after the tag (descriptions are skipped)
        i = (i + 1).max(p.pos.min(p.toks.len()));
        while i < p.toks.len() && p.toks[i].kind != Kind::Tag {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::syntax_errors;

    fn errors(doc: &str) -> Vec<&str> {
        syntax_errors(doc).into_iter().map(|(s, e)| &doc[s..e]).collect()
    }

    #[test]
    fn reports_like_intelephense() {
        assert_eq!(errors("/** @param array< $x */"), vec!["*/"]);
        assert_eq!(errors("/** @return int| */"), vec!["*/"]);
        assert_eq!(errors("/** @var { */"), vec!["{"]);
        assert_eq!(errors("/** @param (int $x */"), vec!["$x"]);
        assert_eq!(errors("/** @param array{a: int $x */"), vec!["$x"]);
        assert_eq!(errors("/**\n * @param array< $x the x\n * @return int\n */"), vec!["the"]);
        assert_eq!(errors("/**\n * @param int||string $x\n */"), vec!["|"]);
        assert_eq!(errors("/**\n * @param array<int, string>\n * @return void\n */"), vec!["@return"]);
        assert_eq!(errors("/** @param int */"), vec!["*/"]);
        assert_eq!(errors("/**\n * @method foo(\n */"), vec!["*/"]);
        assert_eq!(errors("/** @return */"), vec!["*/"]);
    }

    #[test]
    fn accepts_valid_types() {
        for doc in [
            "/**\n * @param array<int, array{a: int, 'b c'?: string}> $a\n * @param callable(int, string): void $b\n * @param \\Closure(): int $c\n * @param int<0, max> $d\n * @param value-of<A::*> $e\n * @param 'a'|'b'|\"c\" $f\n * @param list{int, string} $g\n * @param (A|B)[] $h\n * @param non-empty-array<int>|null $i\n * @param object{a: int} $j\n * @param array<string>[] $k\n * @param ?int $l\n * @param A::CONST_* $n\n * @param -1|1.5 $q\n * @return ($a is int ? string : bool)\n */",
            "/**\n * @param int The value\n * @param int &$x\n * @param string ...$y\n * @param int | string $z\n * @param array< int , string > $w\n */",
            "/**\n * @method static int foo(string $a, int ...$b)\n * @method void bar()\n * @property-read int $p\n * @property int|null $q Some text\n * @template T of \\Countable\n * @template-covariant U\n */",
            "/**\n * @param string $x <b>bold</b> (text\n * @return array A list of {things}\n * @throws \\Exception When it fails\n * @param $untyped\n * @return array{\n *     foo: int,\n *     bar?: string,\n * }\n */",
            "/** @var array<int, string> $x */",
            "/**\n * @ORM\\Column(type=\"string\")\n * {@inheritdoc}\n * @see foo()\n */",
        ] {
            assert!(errors(doc).is_empty(), "{doc}: {:?}", errors(doc));
        }
    }
}
