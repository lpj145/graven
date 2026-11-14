//! Results of the validation rules declared in a schema (`range`, `len`, `finite`, `known`,
//! `check(name)`).
//!
//! Validation is separate from `verify`: `verify` checks that bytes form a well-formed
//! message, while validation checks that its values follow the schema's rules. Generated code
//! exposes it as `XVerified::validate(&checks)` and `XW::to_bytes_validated`. Reading never
//! runs it implicitly, so data written under older rules stays readable.

use core::fmt;

use crate::Error;

/// The rule a value broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// `range(min = …, max = …)`: a number outside its bounds (NaN is always outside).
    Range,
    /// `len(min = …, max = …)`: a string or `bytes` length (in bytes), or a list's element
    /// count, outside its bounds.
    Len,
    /// `finite`: a float that is NaN or infinite.
    Finite,
    /// `known`: an enum value the reader's schema does not declare.
    Known,
    /// `check(name)`: the application's function `name` returned `false`.
    Check(&'static str),
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rule::Range => f.write_str("range"),
            Rule::Len => f.write_str("len"),
            Rule::Finite => f.write_str("finite"),
            Rule::Known => f.write_str("known"),
            Rule::Check(name) => write!(f, "check({name})"),
        }
    }
}

/// A value that breaks a rule of the schema.
///
/// `table` and `field` name where the rule is declared (`field` is `None` for a check on the
/// whole table). `index` is the position in the nearest enclosing list: the element that
/// failed in a list of enums, or the element of a list of tables in which the failing value
/// sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Invalid {
    pub table: &'static str,
    pub field: Option<&'static str>,
    pub index: Option<u32>,
    pub rule: Rule,
}

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.table)?;
        if let Some(field) = self.field {
            write!(f, ".{field}")?;
        }
        if let Some(i) = self.index {
            write!(f, "[{i}]")?;
        }
        write!(f, ": breaks rule `{}`", self.rule)
    }
}

impl core::error::Error for Invalid {}

/// The error of `validate` and `to_bytes_validated`: a rule was broken, or the message could
/// not be read or written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidateError {
    Invalid(Invalid),
    Read(Error),
}

impl ValidateError {
    /// Records the list position `i` on a failure that does not have one yet (used by
    /// generated code when it descends into the elements of a list).
    #[must_use]
    pub fn at(self, i: usize) -> Self {
        match self {
            ValidateError::Invalid(mut v) if v.index.is_none() => {
                v.index = u32::try_from(i).ok();
                ValidateError::Invalid(v)
            }
            other => other,
        }
    }
}

impl From<Invalid> for ValidateError {
    fn from(v: Invalid) -> Self {
        ValidateError::Invalid(v)
    }
}

impl From<Error> for ValidateError {
    fn from(e: Error) -> Self {
        ValidateError::Read(e)
    }
}

impl fmt::Display for ValidateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidateError::Invalid(v) => v.fmt(f),
            ValidateError::Read(e) => e.fmt(f),
        }
    }
}

impl core::error::Error for ValidateError {}
