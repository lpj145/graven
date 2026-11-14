use core::fmt;

/// Every failure mode of reading or writing a Graven buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// A length, header or element extends past the end of the buffer.
    OutOfBounds,
    /// A field slot is only partially covered by the table's `fixed_len`.
    Truncated,
    /// A pointer targets a position outside the buffer.
    BadOffset,
    /// A string is not valid UTF-8.
    InvalidUtf8,
    /// A field required by the reader's schema is absent.
    Missing { field: &'static str },
    /// `verify` exceeded its maximum nesting depth.
    DepthExceeded,
    /// `verify` exceeded its visited-bytes budget.
    BudgetExceeded,
    /// The message would exceed `i32::MAX` bytes.
    TooLarge,
    /// The envelope does not start with the Graven magic bytes.
    BadMagic,
    /// The envelope version is not supported by this reader.
    UnsupportedVersion(u8),
    /// The envelope header is malformed.
    BadHeader,
    /// The input is shorter than the envelope needs; at least `needed` bytes are required.
    Incomplete { needed: usize },
    /// The envelope's schema id differs from the one the reader expects.
    SchemaMismatch { expected: u64, found: u64 },
    /// The envelope's CRC32C does not match its payload.
    ChecksumMismatch,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::OutOfBounds => f.write_str("out of bounds"),
            Error::Truncated => f.write_str("field truncated by fixed_len"),
            Error::BadOffset => f.write_str("pointer outside buffer"),
            Error::InvalidUtf8 => f.write_str("invalid utf-8"),
            Error::Missing { field } => write!(f, "required field `{field}` missing"),
            Error::DepthExceeded => f.write_str("verify depth exceeded"),
            Error::BudgetExceeded => f.write_str("verify budget exceeded"),
            Error::TooLarge => f.write_str("message exceeds i32::MAX bytes"),
            Error::BadMagic => f.write_str("bad magic"),
            Error::UnsupportedVersion(v) => write!(f, "unsupported version {v}"),
            Error::BadHeader => f.write_str("malformed header"),
            Error::Incomplete { needed } => write!(f, "incomplete input: need at least {needed} bytes"),
            Error::SchemaMismatch { expected, found } => {
                write!(f, "schema mismatch: expected {expected:#x}, found {found:#x}")
            }
            Error::ChecksumMismatch => f.write_str("checksum mismatch"),
        }
    }
}

impl core::error::Error for Error {}
