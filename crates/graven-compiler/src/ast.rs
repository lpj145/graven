//! AST of the `.graven` language.

use std::fmt;

/// Position in the source (1-based; `col` counts characters).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    pub namespace: (String, Span),
    /// `import a.b;` / `import a.b as x;`, in order.
    pub imports: Vec<Import>,
    pub items: Vec<Item>,
    /// Documentation comments (`///`), keyed by what they document: `Type` for a type,
    /// `Type.member` for a field, an enum variant or a union variant. Lines are joined with `\n`.
    pub docs: std::collections::BTreeMap<String, String>,
}

/// `import <namespace> [as <alias>];`
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub namespace: String,
    /// The name the file uses: the alias, or the namespace's last segment.
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Enum(EnumDecl),
    Struct(StructDecl),
    Table(TableDecl),
    Union(UnionDecl),
    Service(ServiceDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceDecl {
    pub name: String,
    pub attrs: Vec<Attr>,
    pub methods: Vec<MethodDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MethodDecl {
    pub id: u16,
    /// `None` when the method is `@n: reserved;`.
    pub name: Option<String>,
    /// `None` when reserved.
    pub input: Option<TypeRef>,
    /// `None` when one-way or reserved.
    pub reply: Option<ReplyDecl>,
    pub attrs: Vec<Attr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReplyDecl {
    pub output: TypeRef,
    pub stream: bool,
    pub error: Option<TypeRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub name: String,
    pub base: TypeRef,
    pub values: Vec<(String, i128, Span)>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttrArg {
    pub name: String,
    pub value: Option<Literal>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attr {
    pub name: String,
    pub args: Vec<AttrArg>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub name: String,
    pub attrs: Vec<Attr>,
    pub fields: Vec<(String, TypeRef, Span)>,
    pub field_attrs: Vec<Vec<Attr>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableDecl {
    pub name: String,
    pub attrs: Vec<Attr>,
    pub fields: Vec<FieldDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    pub id: u16,
    /// `None` = a `reserved` field.
    pub name: Option<String>,
    pub ty: Option<TypeRef>,
    pub optional: bool,
    pub default: Option<Literal>,
    pub attrs: Vec<Attr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnionDecl {
    pub name: String,
    pub variants: Vec<(String, u16, TypeRef, Span)>,
    pub span: Span,
}

/// `u8..i128`, `f32`, `f64`, `bool`, `string`, `bytes` are also `Named`. An imported type is
/// `Named("common.Uuid")` (import name, dot, type name).
#[derive(Debug, Clone, PartialEq)]
pub enum TypeRef {
    Named(String),
    List(Box<TypeRef>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    /// An integer in the `i128` range (negative values included).
    Int(i128),
    /// A positive integer above `i128::MAX` (only valid for `u128`).
    UInt(u128),
    Float(f64),
    Bool(bool),
    Ident(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub span: Span,
    pub msg: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.col, self.msg)
    }
}

impl std::error::Error for ParseError {}
