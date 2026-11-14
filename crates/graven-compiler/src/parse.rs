//! Lexer and recursive-descent parser of the `.graven` language (no dependencies).
//!
//! Grammar (EBNF):
//! ```text
//! schema    = "namespace" path ";" { import } { item } ;
//! import    = "import" path [ "as" ident ] ";" ;
//! path      = ident { "." ident } ;
//! item      = enum | struct | table | union ;
//! enum      = "enum" ident ":" type "{" [ evalue { "," evalue } [","] ] "}" ;
//! evalue    = ident "=" int ;                    (* decimal or 0x…, unsigned *)
//! struct    = "struct" ident "{" [ sfield { "," sfield } [","] ] "}" ;
//! sfield    = ident ":" type ;
//! table     = "table" ident "{" { tfield } "}" ;
//! tfield    = { attr } ( "@" int ":" "reserved" [ type ] ";"
//!                      | ident "@" int ":" type [ "?" ] [ "=" literal ] ";" ) ;
//! attr      = "#" "[" ident "]" ;
//! union     = "union" ident "{" [ uvar { "," uvar } [","] ] "}" ;
//! uvar      = ident "@" int ":" type ;
//! type      = ident [ "." ident ] | "[" type "]" ;
//! literal   = [ "-" ] int | [ "-" ] float | "true" | "false" | ident ;
//! comments: `// …\n` and `/* … */`; `/// …` documents the declaration that follows it
//! ```

use crate::ast::*;

pub fn parse(src: &str) -> Result<Schema, ParseError> {
    let toks = lex(src)?;
    Parser { toks, pos: 0, depth: 0, docs: Default::default() }.schema()
}

/// Reads a standalone type (`type` in the grammar), with nothing after it: `"u32"`, `"[Leaf]"`.
/// Used for the canonical type text coming from the lock.
pub fn parse_type(src: &str) -> Result<TypeRef, ParseError> {
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0, depth: 0, docs: Default::default() };
    let t = p.ty()?;
    if *p.peek() != Tok::Eof {
        return p.unexpected("the end of the type");
    }
    Ok(t)
}

// ---------------------------------------------------------------- lexer

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    /// Integer magnitude; the sign is a separate `-` token.
    Int(u128),
    /// One `///` documentation line, without the slashes and one leading space.
    Doc(String),
    Float(f64),
    Punct(char),
    Eof,
}

impl Tok {
    fn describe(&self) -> String {
        match self {
            Tok::Ident(s) => format!("`{s}`"),
            Tok::Int(n) => format!("`{n}`"),
            Tok::Float(x) => format!("`{x}`"),
            Tok::Punct(c) => format!("'{c}'"),
            Tok::Eof => "end of file".into(),
            Tok::Doc(_) => "a documentation comment (`///`)".into(),
        }
    }
}

struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    line: u32,
    col: u32,
}

impl Lexer<'_> {
    fn span(&self) -> Span {
        Span { line: self.line, col: self.col }
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.next()?;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn lookahead(&self, n: usize) -> Option<char> {
        self.chars.clone().nth(n)
    }
}

fn err<T>(span: Span, msg: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError { span, msg: msg.into() })
}

fn lex(src: &str) -> Result<Vec<(Tok, Span)>, ParseError> {
    let mut lx = Lexer { chars: src.chars().peekable(), line: 1, col: 1 };
    let mut out = Vec::new();
    while let Some(c) = lx.peek() {
        let sp = lx.span();
        if c.is_whitespace() {
            lx.bump();
        } else if c == '/' && lx.lookahead(1) == Some('/') {
            let mut line = String::new();
            while let Some(c) = lx.peek() {
                if c == '\n' {
                    break;
                }
                line.push(c);
                lx.bump();
            }
            // `///` documents the next declaration; `////` and longer are plain comments.
            if let Some(text) = line.strip_prefix("///").filter(|t| !t.starts_with('/')) {
                out.push((Tok::Doc(text.strip_prefix(' ').unwrap_or(text).trim_end().to_string()), sp));
            }
        } else if c == '/' && lx.lookahead(1) == Some('*') {
            lx.bump();
            lx.bump();
            loop {
                match lx.bump() {
                    None => return err(sp, "unterminated block comment"),
                    Some('*') if lx.peek() == Some('/') => {
                        lx.bump();
                        break;
                    }
                    Some(_) => {}
                }
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            let mut s = String::new();
            while let Some(c) = lx.peek() {
                if c.is_ascii_alphanumeric() || c == '_' {
                    s.push(c);
                    lx.bump();
                } else {
                    break;
                }
            }
            out.push((Tok::Ident(s), sp));
        } else if c.is_ascii_digit() {
            out.push((lex_number(&mut lx)?, sp));
        } else if "{}[](),;:@?=#.-".contains(c) {
            lx.bump();
            out.push((Tok::Punct(c), sp));
        } else {
            return err(sp, format!("unexpected character {c:?}"));
        }
    }
    out.push((Tok::Eof, lx.span()));
    Ok(out)
}

fn lex_number(lx: &mut Lexer) -> Result<Tok, ParseError> {
    let sp = lx.span();
    let first = lx.bump().unwrap();
    if first == '0' && matches!(lx.peek(), Some('x' | 'X')) {
        lx.bump();
        let mut digits = String::new();
        while let Some(c) = lx.peek() {
            if c.is_ascii_hexdigit() || c == '_' {
                digits.push(c);
                lx.bump();
            } else {
                break;
            }
        }
        let digits = digits.replace('_', "");
        if digits.is_empty() {
            return err(sp, "hexadecimal literal without digits");
        }
        return u128::from_str_radix(&digits, 16)
            .map(Tok::Int)
            .or_else(|_| err(sp, "integer literal out of the 128-bit range"));
    }
    let mut text = String::from(first);
    let mut is_float = false;
    while let Some(c) = lx.peek() {
        if c.is_ascii_digit() || c == '_' {
            text.push(c);
            lx.bump();
        } else if c == '.' && !is_float && lx.lookahead(1).is_some_and(|d| d.is_ascii_digit()) {
            is_float = true;
            text.push(c);
            lx.bump();
        } else if matches!(c, 'e' | 'E') {
            let n1 = lx.lookahead(1);
            let signed = matches!(n1, Some('+' | '-'));
            let digit_at = if signed { 2 } else { 1 };
            if !lx.lookahead(digit_at).is_some_and(|d| d.is_ascii_digit()) {
                break;
            }
            is_float = true;
            text.push(c);
            lx.bump();
            if signed {
                text.push(lx.bump().unwrap());
            }
        } else {
            break;
        }
    }
    let text = text.replace('_', "");
    if is_float {
        let x: f64 = text.parse().or_else(|_| err(sp, "invalid floating-point literal"))?;
        if x.is_infinite() {
            return err(sp, format!("floating-point literal {text} is too large for f64"));
        }
        // A non-zero literal that rounds to 0 would silently become another value.
        let mantissa = text.split(['e', 'E']).next().unwrap_or("");
        if x == 0.0 && mantissa.bytes().any(|c| (b'1'..=b'9').contains(&c)) {
            return err(sp, format!("floating-point literal {text} is too small for f64: it would become 0"));
        }
        Ok(Tok::Float(x))
    } else {
        text.parse::<u128>()
            .map(Tok::Int)
            .or_else(|_| err(sp, "integer literal out of the 128-bit range"))
    }
}

// --------------------------------------------------------------- parser

const MAX_DEPTH: u32 = 64;

struct Parser {
    toks: Vec<(Tok, Span)>,
    pos: usize,
    depth: u32,
    docs: std::collections::BTreeMap<String, String>,
}

/// Documentation lines waiting for the declaration that follows them.
type PendingDoc = Option<(String, Span)>;

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].0
    }

    fn span(&self) -> Span {
        self.toks[self.pos].1
    }

    fn bump(&mut self) -> (Tok, Span) {
        let t = self.toks[self.pos].clone();
        if t.0 != Tok::Eof {
            self.pos += 1;
        }
        t
    }

    fn is_punct(&self, c: char) -> bool {
        *self.peek() == Tok::Punct(c)
    }

    fn eat_punct(&mut self, c: char) -> bool {
        let hit = self.is_punct(c);
        if hit {
            self.bump();
        }
        hit
    }

    fn unexpected<T>(&self, expected: &str) -> Result<T, ParseError> {
        err(self.span(), format!("expected {expected}, found {}", self.peek().describe()))
    }

    fn expect_punct(&mut self, c: char) -> Result<Span, ParseError> {
        if self.is_punct(c) {
            Ok(self.bump().1)
        } else {
            self.unexpected(&format!("'{c}'"))
        }
    }

    fn ident(&mut self, what: &str) -> Result<(String, Span), ParseError> {
        match self.peek() {
            Tok::Ident(_) => {
                let (Tok::Ident(s), sp) = self.bump() else { unreachable!() };
                Ok((s, sp))
            }
            _ => self.unexpected(what),
        }
    }

    fn keyword(&mut self, kw: &str) -> Result<Span, ParseError> {
        match self.peek() {
            Tok::Ident(s) if s == kw => Ok(self.bump().1),
            _ => self.unexpected(&format!("`{kw}`")),
        }
    }

    /// Unsigned integer (decimal or hexadecimal).
    fn uint(&mut self, what: &str) -> Result<(u128, Span), ParseError> {
        match self.peek() {
            Tok::Int(_) => {
                let (Tok::Int(n), sp) = self.bump() else { unreachable!() };
                Ok((n, sp))
            }
            _ => self.unexpected(what),
        }
    }

    /// Consecutive `///` lines, joined with `\n`.
    fn take_docs(&mut self) -> PendingDoc {
        let mut out: PendingDoc = None;
        while let Tok::Doc(_) = self.peek() {
            let (Tok::Doc(line), sp) = self.bump() else { unreachable!() };
            match &mut out {
                Some((text, _)) => {
                    text.push('\n');
                    text.push_str(&line);
                }
                None => out = Some((line, sp)),
            }
        }
        out
    }

    /// Attaches pending documentation to `key` (`Type` or `Type.member`).
    fn attach(&mut self, key: String, doc: PendingDoc) {
        if let Some((text, _)) = doc {
            self.docs.insert(key, text);
        }
    }

    /// Documentation with no declaration after it is an error, not silently dropped.
    fn no_dangling(doc: &PendingDoc) -> Result<(), ParseError> {
        match doc {
            Some((_, sp)) => err(*sp, "documentation comment (`///`) without a declaration after it"),
            None => Ok(()),
        }
    }

    fn id_u16(&mut self) -> Result<u16, ParseError> {
        let (n, sp) = self.uint("an integer (id)")?;
        u16::try_from(n).or_else(|_| err(sp, format!("id {n} out of the u16 range")))
    }

    fn schema(&mut self) -> Result<Schema, ParseError> {
        self.keyword("namespace")?;
        let nsp = self.span();
        let (mut ns, _) = self.ident("the namespace name")?;
        while self.eat_punct('.') {
            ns.push('.');
            ns.push_str(&self.ident("a namespace segment")?.0);
        }
        self.expect_punct(';')?;
        let mut imports = Vec::new();
        while matches!(self.peek(), Tok::Ident(s) if s == "import") {
            let span = self.bump().1;
            let (mut path, _) = self.ident("the imported namespace")?;
            while self.eat_punct('.') {
                path.push('.');
                path.push_str(&self.ident("a namespace segment")?.0);
            }
            let name = if matches!(self.peek(), Tok::Ident(s) if s == "as") {
                self.bump();
                self.ident("the import alias")?.0
            } else {
                path.rsplit('.').next().unwrap_or(&path).to_string()
            };
            self.expect_punct(';')?;
            imports.push(Import { namespace: path, name, span });
        }
        let mut items = Vec::new();
        loop {
            let doc = self.take_docs();
            if *self.peek() == Tok::Eof {
                Self::no_dangling(&doc)?;
                break;
            }
            items.push(self.item(doc)?);
        }
        Ok(Schema { namespace: (ns, nsp), imports, items, docs: std::mem::take(&mut self.docs) })
    }

    fn item(&mut self, doc: PendingDoc) -> Result<Item, ParseError> {
        let attrs = self.attrs()?;
        let kw = match self.peek() {
            Tok::Ident(s) => s.clone(),
            _ => String::new(),
        };
        let item = match kw.as_str() {
            "enum" => {
                if let Some(a) = attrs.first() {
                    return err(a.span, "attributes are not allowed on enums");
                }
                self.enum_decl().map(Item::Enum)
            }
            "struct" => self.struct_decl(attrs).map(Item::Struct),
            "table" => self.table_decl(attrs).map(Item::Table),
            "union" => {
                if let Some(a) = attrs.first() {
                    return err(a.span, "attributes are not allowed on unions");
                }
                self.union_decl().map(Item::Union)
            }
            _ => self.unexpected("`enum`, `struct`, `table` or `union`"),
        }?;
        let name = match &item {
            Item::Enum(d) => &d.name,
            Item::Struct(d) => &d.name,
            Item::Table(d) => &d.name,
            Item::Union(d) => &d.name,
        };
        self.attach(name.clone(), doc);
        Ok(item)
    }

    fn ty(&mut self) -> Result<TypeRef, ParseError> {
        if self.eat_punct('[') {
            if self.depth >= MAX_DEPTH {
                return err(self.span(), "type nested too deeply");
            }
            self.depth += 1;
            let inner = self.ty();
            self.depth -= 1;
            let inner = inner?;
            self.expect_punct(']')?;
            Ok(TypeRef::List(Box::new(inner)))
        } else {
            let (mut name, _) = self.ident("a type")?;
            // `import_name.Type`: a type from an imported namespace.
            if self.eat_punct('.') {
                name.push('.');
                name.push_str(&self.ident("the imported type name")?.0);
            }
            Ok(TypeRef::Named(name))
        }
    }

    fn enum_decl(&mut self) -> Result<EnumDecl, ParseError> {
        let span = self.keyword("enum")?;
        let (name, _) = self.ident("the enum name")?;
        self.expect_punct(':')?;
        let base = self.ty()?;
        self.expect_punct('{')?;
        let mut values = Vec::new();
        loop {
            let doc = self.take_docs();
            if self.is_punct('}') {
                Self::no_dangling(&doc)?;
                break;
            }
            let (vn, vsp) = self.ident("the variant name")?;
            self.attach(format!("{name}.{vn}"), doc);
            self.expect_punct('=')?;
            let (n, _) = self.uint("an integer value")?;
            // Enum bases are at most u32, so anything above i128 is rejected by `check` anyway.
            values.push((vn, i128::try_from(n).unwrap_or(i128::MAX), vsp));
            if !self.eat_punct(',') {
                break;
            }
        }
        self.expect_punct('}')?;
        Ok(EnumDecl { name, base, values, span })
    }

    fn struct_decl(&mut self, attrs: Vec<Attr>) -> Result<StructDecl, ParseError> {
        let span = self.keyword("struct")?;
        let (name, _) = self.ident("the struct name")?;
        self.expect_punct('{')?;
        let mut fields = Vec::new();
        let mut field_attrs = Vec::new();
        loop {
            let doc = self.take_docs();
            if self.is_punct('}') {
                Self::no_dangling(&doc)?;
                break;
            }
            let fattrs = self.attrs()?;
            let (fname, fsp) = self.ident("the field name")?;
            self.attach(format!("{name}.{fname}"), doc);
            self.expect_punct(':')?;
            let ty = self.ty()?;
            fields.push((fname, ty, fsp));
            field_attrs.push(fattrs);
            if !self.eat_punct(',') {
                break;
            }
        }
        self.expect_punct('}')?;
        Ok(StructDecl { name, attrs, fields, field_attrs, span })
    }

    fn union_decl(&mut self) -> Result<UnionDecl, ParseError> {
        let span = self.keyword("union")?;
        let (name, _) = self.ident("the union name")?;
        self.expect_punct('{')?;
        let mut variants = Vec::new();
        loop {
            let doc = self.take_docs();
            if self.is_punct('}') {
                Self::no_dangling(&doc)?;
                break;
            }
            let (vn, vsp) = self.ident("the variant name")?;
            self.attach(format!("{name}.{vn}"), doc);
            self.expect_punct('@')?;
            let id = self.id_u16()?;
            self.expect_punct(':')?;
            let ty = self.ty()?;
            variants.push((vn, id, ty, vsp));
            if !self.eat_punct(',') {
                break;
            }
        }
        self.expect_punct('}')?;
        Ok(UnionDecl { name, variants, span })
    }

    fn table_decl(&mut self, attrs: Vec<Attr>) -> Result<TableDecl, ParseError> {
        let span = self.keyword("table")?;
        let (name, _) = self.ident("the table name")?;
        self.expect_punct('{')?;
        let mut fields = Vec::new();
        loop {
            let doc = self.take_docs();
            if self.is_punct('}') {
                Self::no_dangling(&doc)?;
                break;
            }
            let f = self.table_field()?;
            if let Some(n) = &f.name {
                self.attach(format!("{name}.{n}"), doc);
            }
            fields.push(f);
        }
        self.expect_punct('}')?;
        Ok(TableDecl { name, attrs, fields, span })
    }

    fn attrs(&mut self) -> Result<Vec<Attr>, ParseError> {
        let mut attrs = Vec::new();
        while self.is_punct('#') {
            self.bump();
            self.expect_punct('[')?;
            loop {
                attrs.push(self.attr()?);
                if !self.eat_punct(',') {
                    break;
                }
                if self.is_punct(']') {
                    break;
                }
            }
            self.expect_punct(']')?;
        }
        Ok(attrs)
    }

    fn attr(&mut self) -> Result<Attr, ParseError> {
        let (name, span) = self.ident("the attribute name")?;
        let mut args = Vec::new();
        if self.eat_punct('(') {
            if !self.is_punct(')') {
                loop {
                    let (aname, aspan) = self.ident("the argument name")?;
                    let value = if self.eat_punct('=') { Some(self.literal()?) } else { None };
                    args.push(AttrArg { name: aname, value, span: aspan });
                    if !self.eat_punct(',') {
                        break;
                    }
                    if self.is_punct(')') {
                        break;
                    }
                }
            }
            self.expect_punct(')')?;
        }
        Ok(Attr { name, args, span })
    }

    fn table_field(&mut self) -> Result<FieldDecl, ParseError> {
        let attrs = self.attrs()?;
        let span = self.span();
        if self.eat_punct('@') {
            let id = self.id_u16()?;
            self.expect_punct(':')?;
            self.keyword("reserved")?;
            let ty = if self.is_punct(';') { None } else { Some(self.ty()?) };
            self.expect_punct(';')?;
            return Ok(FieldDecl {
                id,
                name: None,
                ty,
                optional: false,
                default: None,
                attrs,
                span,
            });
        }
        let (fname, _) = self.ident("the field name or `@id`")?;
        self.expect_punct('@')?;
        let id = self.id_u16()?;
        self.expect_punct(':')?;
        let ty = self.ty()?;
        let optional = self.eat_punct('?');
        let default = if self.eat_punct('=') { Some(self.literal()?) } else { None };
        self.expect_punct(';')?;
        Ok(FieldDecl { id, name: Some(fname), ty: Some(ty), optional, default, attrs, span })
    }

    fn literal(&mut self) -> Result<Literal, ParseError> {
        let neg = self.eat_punct('-');
        match self.peek().clone() {
            Tok::Int(n) => {
                let (_, sp) = self.bump();
                match (neg, i128::try_from(n)) {
                    (false, Ok(i)) => Ok(Literal::Int(i)),
                    (false, Err(_)) => Ok(Literal::UInt(n)),
                    (true, Ok(i)) => Ok(Literal::Int(-i)),
                    // `-2^127` is `i128::MIN`; anything beyond is out of range.
                    (true, Err(_)) if n == i128::MIN.unsigned_abs() => Ok(Literal::Int(i128::MIN)),
                    (true, Err(_)) => err(sp, format!("integer literal -{n} is below the i128 range")),
                }
            }
            Tok::Float(x) => {
                self.bump();
                Ok(Literal::Float(if neg { -x } else { x }))
            }
            Tok::Ident(s) if !neg => {
                self.bump();
                Ok(match s.as_str() {
                    "true" => Literal::Bool(true),
                    "false" => Literal::Bool(false),
                    _ => Literal::Ident(s),
                })
            }
            _ => self.unexpected("a literal (integer, float, bool or identifier)"),
        }
    }
}
