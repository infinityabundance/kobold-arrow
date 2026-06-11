//! # kobold-arrow
//!
//! The COBOL fixed-record -> Apache Arrow **schema mapper**. Given a declared COBOL record layout
//! (fields tagged with their physical COBOL encoding), it produces the equivalent Apache Arrow
//! schema -- field name, Arrow logical [`ArrowType`], and nullability -- so a legacy fixed-record
//! dataset can be landed into the Arrow / columnar world with a faithful, lossless type mapping.
//!
//! The schema is emitted as a plain serializable description whose type names mirror Arrow's own
//! logical types ([`ArrowType::Utf8`], [`ArrowType::Decimal128`], ...). This crate deliberately does
//! **not** link the heavyweight `arrow` crate: it is a light, dependency-minimal mapper that produces
//! a schema *as data*. Building actual `RecordBatch` columns on top of the real `arrow` crate is a
//! documented future feature (see README).
//!
//! Part of the KOBOLD ecosystem -- independently-authored forensic tooling, Apache-2.0. This crate
//! contains **no GnuCOBOL/libcob source** and depends on nothing but `serde`/`serde_json`; the byte
//! formats it maps (packed-decimal, zoned-decimal, binary, fixed records) are public, long-documented
//! data conventions.
//!
//! ## Mapping rules
//! The mapping is intentionally conservative and lossless -- it never narrows a value range:
//! - `PIC X(n)` ([`CobolEncoding::Alphanumeric`]) -> [`ArrowType::Utf8`].
//! - DISPLAY or COMP-3 numeric with a fractional part (`scale > 0`) -> [`ArrowType::Decimal128`]
//!   with `precision = digits`, `scale = scale`. Decimals are kept exact; never floated.
//! - Integer DISPLAY or COMP-3 (`scale == 0`): `digits <= 9` -> [`ArrowType::Int32`];
//!   `10..=18` -> [`ArrowType::Int64`]; `> 18` exceeds 64-bit range -> [`ArrowType::Decimal128`]
//!   `{ precision: digits, scale: 0 }`.
//! - Binary ([`CobolEncoding::Binary`], COMP / COMP-5): `bytes <= 4` -> [`ArrowType::Int32`],
//!   otherwise -> [`ArrowType::Int64`]. (Arrow integers are signed; the COBOL `signed` flag is
//!   carried for documentation but does not change the chosen width.)
//! - [`CobolEncoding::Float`] (COMP-1) -> [`ArrowType::Float32`].
//! - [`CobolEncoding::Double`] (COMP-2) -> [`ArrowType::Float64`].
//!
//! COBOL fixed records have no SQL-style NULL, so every mapped field is non-nullable.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// The physical encoding of a COBOL elementary field -- how the declared `PIC`/`USAGE` is stored.
///
/// `digits` is the PIC 9-count (total significant decimal digits); `scale` is the number of those
/// digits to the right of the implied decimal point (`V`); `signed` reflects a leading `S`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum CobolEncoding {
    /// `PIC X(len)` -- opaque alphanumeric text.
    Alphanumeric { len: u32 },
    /// `PIC [S]9(d)[V9(s)]` DISPLAY -- zoned/unpacked decimal stored one digit per byte.
    DisplayNumeric { digits: u32, scale: u32, signed: bool },
    /// `PIC [S]9(d)[V9(s)] COMP-3` -- packed (BCD) decimal.
    Packed { digits: u32, scale: u32, signed: bool },
    /// `PIC [S]9 COMP` / `COMP-5` -- binary integer of `bytes` width.
    Binary { bytes: u32, signed: bool },
    /// `COMP-1` -- single-precision binary floating point.
    Float,
    /// `COMP-2` -- double-precision binary floating point.
    Double,
}

/// A declared COBOL elementary field: its name and physical [`CobolEncoding`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CobolField {
    pub name: String,
    pub encoding: CobolEncoding,
}

/// A COBOL fixed-record layout: a named record and its ordered elementary fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CobolLayout {
    pub name: String,
    #[serde(default)]
    pub fields: Vec<CobolField>,
}

/// An Apache Arrow logical data type -- the subset needed to land fixed-record COBOL data. Variant
/// and field names mirror Arrow's own type vocabulary so the emitted JSON reads as an Arrow schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub enum ArrowType {
    /// Variable-length UTF-8 string. Arrow `Utf8`.
    Utf8,
    /// 32-bit signed integer. Arrow `Int32`.
    Int32,
    /// 64-bit signed integer. Arrow `Int64`.
    Int64,
    /// Fixed-point 128-bit decimal. Arrow `Decimal128(precision, scale)`.
    Decimal128 { precision: u32, scale: u32 },
    /// 32-bit IEEE-754 float. Arrow `Float32`.
    Float32,
    /// 64-bit IEEE-754 float. Arrow `Float64`.
    Float64,
    /// Fixed-width opaque byte buffer. Arrow `FixedSizeBinary(len)`.
    FixedSizeBinary { len: u32 },
    /// Boolean. Arrow `Boolean`. (Not produced by the default mapper; available for callers.)
    Boolean,
}

/// One Arrow schema field: name, logical [`ArrowType`], and nullability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrowField {
    pub name: String,
    pub data_type: ArrowType,
    pub nullable: bool,
}

/// An Arrow schema: the ordered list of [`ArrowField`]s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrowSchema {
    pub fields: Vec<ArrowField>,
}

/// The largest decimal `digits` that always fits in a signed 64-bit integer. `i64::MAX` is 19
/// digits but only `<= 18` all-nines values are guaranteed to fit, so 18 is the safe integer ceiling.
const INT64_MAX_DIGITS: u32 = 18;
/// The largest decimal `digits` that always fits in a signed 32-bit integer (`i32::MAX = 2_147_483_647`).
const INT32_MAX_DIGITS: u32 = 9;

/// Map a single COBOL encoding to its Arrow logical type, following the documented rules.
pub fn map_encoding(enc: &CobolEncoding) -> ArrowType {
    match *enc {
        CobolEncoding::Alphanumeric { .. } => ArrowType::Utf8,
        CobolEncoding::DisplayNumeric { digits, scale, .. }
        | CobolEncoding::Packed { digits, scale, .. } => map_fixed_decimal(digits, scale),
        CobolEncoding::Binary { bytes, .. } => {
            if bytes <= 4 {
                ArrowType::Int32
            } else {
                ArrowType::Int64
            }
        }
        CobolEncoding::Float => ArrowType::Float32,
        CobolEncoding::Double => ArrowType::Float64,
    }
}

/// Shared rule for DISPLAY/COMP-3 fixed-point numerics: scaled -> `Decimal128`; integer -> the
/// narrowest exact integer type, widening to `Decimal128` when `digits` overflows 64-bit range.
fn map_fixed_decimal(digits: u32, scale: u32) -> ArrowType {
    if scale > 0 {
        ArrowType::Decimal128 { precision: digits, scale }
    } else if digits <= INT32_MAX_DIGITS {
        ArrowType::Int32
    } else if digits <= INT64_MAX_DIGITS {
        ArrowType::Int64
    } else {
        ArrowType::Decimal128 { precision: digits, scale: 0 }
    }
}

/// Map a [`CobolField`] to an [`ArrowField`]. COBOL fixed records carry no NULL marker, so the
/// mapped field is non-nullable.
pub fn map_field(field: &CobolField) -> ArrowField {
    ArrowField {
        name: field.name.clone(),
        data_type: map_encoding(&field.encoding),
        nullable: false,
    }
}

/// Map a whole [`CobolLayout`] to an [`ArrowSchema`], field by field, preserving order.
pub fn map_layout(layout: &CobolLayout) -> ArrowSchema {
    ArrowSchema {
        fields: layout.fields.iter().map(map_field).collect(),
    }
}

/// Serialize an [`ArrowSchema`] to compact JSON.
pub fn to_json(schema: &ArrowSchema) -> Result<String, serde_json::Error> {
    serde_json::to_string(schema)
}

/// Serialize an [`ArrowSchema`] to pretty (indented) JSON.
pub fn to_json_pretty(schema: &ArrowSchema) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, enc: CobolEncoding) -> CobolField {
        CobolField { name: name.to_string(), encoding: enc }
    }

    #[test]
    fn pic_x_maps_to_utf8() {
        // PIC X(10)
        let f = map_field(&field("name", CobolEncoding::Alphanumeric { len: 10 }));
        assert_eq!(f.data_type, ArrowType::Utf8);
        assert!(!f.nullable);
    }

    #[test]
    fn scaled_packed_maps_to_decimal128() {
        // PIC S9(7)V99 COMP-3 -> Decimal128(precision: 7, scale: 2)
        let f = map_field(&field(
            "amount",
            CobolEncoding::Packed { digits: 7, scale: 2, signed: true },
        ));
        assert_eq!(f.data_type, ArrowType::Decimal128 { precision: 7, scale: 2 });
    }

    #[test]
    fn scaled_display_maps_to_decimal128() {
        // PIC 9(5)V99 DISPLAY -> Decimal128(precision: 5, scale: 2)
        let f = map_field(&field(
            "rate",
            CobolEncoding::DisplayNumeric { digits: 5, scale: 2, signed: false },
        ));
        assert_eq!(f.data_type, ArrowType::Decimal128 { precision: 5, scale: 2 });
    }

    #[test]
    fn small_integer_maps_to_int32() {
        // PIC 9(5) -> Int32
        let f = map_field(&field(
            "qty",
            CobolEncoding::DisplayNumeric { digits: 5, scale: 0, signed: false },
        ));
        assert_eq!(f.data_type, ArrowType::Int32);
    }

    #[test]
    fn boundary_nine_digits_is_int32() {
        // PIC 9(9) -> Int32 (boundary)
        let t = map_encoding(&CobolEncoding::DisplayNumeric { digits: 9, scale: 0, signed: false });
        assert_eq!(t, ArrowType::Int32);
    }

    #[test]
    fn ten_digits_is_int64() {
        // PIC 9(10) -> Int64
        let t = map_encoding(&CobolEncoding::DisplayNumeric { digits: 10, scale: 0, signed: false });
        assert_eq!(t, ArrowType::Int64);
    }

    #[test]
    fn large_integer_maps_to_int64() {
        // PIC 9(15) -> Int64
        let f = map_field(&field(
            "big",
            CobolEncoding::DisplayNumeric { digits: 15, scale: 0, signed: false },
        ));
        assert_eq!(f.data_type, ArrowType::Int64);
    }

    #[test]
    fn eighteen_digits_is_int64_nineteen_is_decimal() {
        assert_eq!(
            map_encoding(&CobolEncoding::Packed { digits: 18, scale: 0, signed: true }),
            ArrowType::Int64
        );
        // 19 digits overflows i64 range -> Decimal128(scale 0)
        assert_eq!(
            map_encoding(&CobolEncoding::Packed { digits: 19, scale: 0, signed: true }),
            ArrowType::Decimal128 { precision: 19, scale: 0 }
        );
    }

    #[test]
    fn binary_width_selects_int_width() {
        // COMP halfword (2 bytes) -> Int32; fullword (4) -> Int32; doubleword (8) -> Int64
        assert_eq!(
            map_encoding(&CobolEncoding::Binary { bytes: 2, signed: true }),
            ArrowType::Int32
        );
        assert_eq!(
            map_encoding(&CobolEncoding::Binary { bytes: 4, signed: true }),
            ArrowType::Int32
        );
        assert_eq!(
            map_encoding(&CobolEncoding::Binary { bytes: 8, signed: false }),
            ArrowType::Int64
        );
    }

    #[test]
    fn float_and_double_map_to_arrow_floats() {
        assert_eq!(map_encoding(&CobolEncoding::Float), ArrowType::Float32);
        assert_eq!(map_encoding(&CobolEncoding::Double), ArrowType::Float64);
    }

    #[test]
    fn full_layout_maps_every_field() {
        let layout = CobolLayout {
            name: "CUSTOMER-RECORD".to_string(),
            fields: vec![
                field("cust-name", CobolEncoding::Alphanumeric { len: 30 }),
                field("cust-id", CobolEncoding::DisplayNumeric { digits: 5, scale: 0, signed: false }),
                field("balance", CobolEncoding::Packed { digits: 9, scale: 2, signed: true }),
                field("seq", CobolEncoding::Binary { bytes: 4, signed: false }),
                field("ratio", CobolEncoding::Double),
            ],
        };
        let schema = map_layout(&layout);
        assert_eq!(schema.fields.len(), 5);
        assert_eq!(schema.fields[0].data_type, ArrowType::Utf8);
        assert_eq!(schema.fields[1].data_type, ArrowType::Int32);
        assert_eq!(
            schema.fields[2].data_type,
            ArrowType::Decimal128 { precision: 9, scale: 2 }
        );
        assert_eq!(schema.fields[3].data_type, ArrowType::Int32);
        assert_eq!(schema.fields[4].data_type, ArrowType::Float64);
        assert!(schema.fields.iter().all(|f| !f.nullable));
    }

    #[test]
    fn schema_round_trips_through_json() {
        let layout = CobolLayout {
            name: "R".to_string(),
            fields: vec![field("a", CobolEncoding::Alphanumeric { len: 4 })],
        };
        let schema = map_layout(&layout);
        let json = to_json(&schema).expect("serialize");
        let back: ArrowSchema = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(schema, back);
        // The emitted type name mirrors Arrow's vocabulary.
        assert!(json.contains("Utf8"));
    }

    #[test]
    fn layout_json_deserializes_with_kebab_encoding_tags() {
        let src = r#"{
            "name":"REC",
            "fields":[
                {"name":"id","encoding":{"display-numeric":{"digits":5,"scale":0,"signed":false}}},
                {"name":"amt","encoding":{"packed":{"digits":7,"scale":2,"signed":true}}}
            ]
        }"#;
        let layout: CobolLayout = serde_json::from_str(src).expect("parse layout");
        let schema = map_layout(&layout);
        assert_eq!(schema.fields[0].data_type, ArrowType::Int32);
        assert_eq!(
            schema.fields[1].data_type,
            ArrowType::Decimal128 { precision: 7, scale: 2 }
        );
    }
}
