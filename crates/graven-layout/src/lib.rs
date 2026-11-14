//! Deterministic fixed-part layout of Graven tables (`docs/spec/graven-format-v1.md` §3.3),
//! shared by the runtime's tests, the conformance interpreter and the `gravenc` compiler.

/// Wire shape of a field in a table's fixed part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Integer, float, enum or inline struct: `size` bytes + 1 presence bit.
    Scalar { size: u16 },
    /// 2 bits (presence + value), no slot.
    Bool,
    /// string, bytes, list or table: i32 offset, no bit (0 = absent).
    Ptr,
    /// `[u16 tag][i32 offset]`, no bit (tag 0 = absent).
    Union,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldLayout {
    /// Slot offset within the fixed part (None for bool).
    pub slot: Option<u16>,
    /// Presence bit `(byte, shift)`; for bool the value is at `shift + 1`.
    pub presence: Option<(u16, u8)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableLayout {
    pub fixed_len: u16,
    pub fields: Vec<FieldLayout>,
}

impl FieldKind {
    fn bits(self) -> u8 {
        match self {
            FieldKind::Scalar { .. } => 1,
            FieldKind::Bool => 2,
            FieldKind::Ptr | FieldKind::Union => 0,
        }
    }
    fn size(self) -> u16 {
        match self {
            FieldKind::Scalar { size } => size,
            FieldKind::Bool => 0,
            FieldKind::Ptr => 4,
            FieldKind::Union => 6,
        }
    }
}

/// Layout for `fields` in ID order, including reserved fields with their original kind.
pub fn compute_layout(fields: &[FieldKind]) -> TableLayout {
    let mut cursor: u16 = 0;
    let mut open: Option<(u16, u8)> = None;
    let mut out = Vec::with_capacity(fields.len());
    for &f in fields {
        let need = f.bits();
        let presence = if need > 0 {
            match open {
                Some((p, s)) if s + need <= 8 => {
                    open = Some((p, s + need));
                    Some((p, s))
                }
                _ => {
                    let p = cursor;
                    cursor += 1;
                    open = Some((p, need));
                    Some((p, 0))
                }
            }
        } else {
            None
        };
        let slot = if f == FieldKind::Bool {
            None
        } else {
            let s = cursor;
            cursor += f.size();
            Some(s)
        };
        out.push(FieldLayout { slot, presence });
    }
    TableLayout { fixed_len: cursor, fields: out }
}

#[cfg(test)]
mod tests {
    use super::*;
    use FieldKind::*;

    #[test]
    fn telemetry_layout() {
        let l = compute_layout(&[Scalar { size: 4 }, Scalar { size: 8 }, Scalar { size: 4 }, Scalar { size: 4 }, Scalar { size: 1 }]);
        assert_eq!(l.fixed_len, 22);
        let slots: Vec<_> = l.fields.iter().map(|f| f.slot.unwrap()).collect();
        assert_eq!(slots, [1, 5, 13, 17, 21]);
        let bits: Vec<_> = l.fields.iter().map(|f| f.presence.unwrap()).collect();
        assert_eq!(bits, [(0, 0), (0, 1), (0, 2), (0, 3), (0, 4)]);
    }

    #[test]
    fn bool_does_not_split_across_bytes() {
        let mut f = vec![Scalar { size: 1 }; 7];
        f.push(Bool);
        let l = compute_layout(&f);
        // 7 scalars: bits byte 0 (shifts 0..6), slots 1..=7; bool needs 2 bits -> new byte at 8
        assert_eq!(l.fields[7].presence, Some((8, 0)));
        assert_eq!(l.fields[7].slot, None);
        assert_eq!(l.fixed_len, 9);
    }

    #[test]
    fn bool_fits_in_open_byte() {
        let l = compute_layout(&[Scalar { size: 2 }, Bool]);
        assert_eq!(l.fields[1].presence, Some((0, 1)));
        assert_eq!(l.fixed_len, 3);
    }

    #[test]
    fn ptr_has_no_bit() {
        let l = compute_layout(&[Ptr, Scalar { size: 2 }]);
        assert_eq!(l.fields[0], FieldLayout { slot: Some(0), presence: None });
        assert_eq!(l.fields[1], FieldLayout { slot: Some(5), presence: Some((4, 0)) });
        assert_eq!(l.fixed_len, 7);
    }

    #[test]
    fn union_is_six_bytes() {
        let l = compute_layout(&[Union, Ptr]);
        assert_eq!(l.fields[1].slot, Some(6));
        assert_eq!(l.fixed_len, 10);
    }

    #[test]
    fn append_is_stable() {
        let v1 = [Scalar { size: 4 }, Ptr, Bool, Scalar { size: 2 }];
        let mut v2 = v1.to_vec();
        v2.extend([Scalar { size: 8 }, Bool, Ptr]);
        let (a, b) = (compute_layout(&v1), compute_layout(&v2));
        assert_eq!(&b.fields[..v1.len()], &a.fields[..]);
        assert!(b.fixed_len >= a.fixed_len);
    }
}
