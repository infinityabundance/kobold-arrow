# kobold-arrow

Map COBOL fixed-record layouts to equivalent **Apache Arrow** schemas.

Given a declared COBOL record layout (fields tagged with their physical COBOL encoding -- `PIC X`,
DISPLAY numeric, COMP-3 packed decimal, COMP/COMP-5 binary, COMP-1/COMP-2 float), kobold-arrow
produces the equivalent Arrow schema: each field's name, Arrow logical type, and nullability. This
lets a legacy fixed-record dataset be landed into the Arrow / columnar world with a faithful,
lossless type mapping.

The schema is emitted **as data** -- a serializable description whose type names mirror Arrow's own
logical vocabulary (`Utf8`, `Int32`, `Int64`, `Decimal128`, `Float32`, `Float64`, ...). The crate
deliberately does **not** link the heavyweight `arrow` crate; it depends only on `serde`/`serde_json`
and stays light.

**Part of KOBOLD** -- a forensic archaeology and evidence system for legacy COBOL estates.
Independently-authored tooling; contains no GnuCOBOL source.

## Mapping rules
- `PIC X(n)` -> `Utf8`
- DISPLAY / COMP-3 numeric with `scale > 0` -> `Decimal128(precision = digits, scale)` (kept exact)
- Integer DISPLAY / COMP-3 (`scale == 0`): `digits <= 9` -> `Int32`; `10..=18` -> `Int64`;
  `> 18` -> `Decimal128(precision = digits, scale = 0)`
- Binary (COMP / COMP-5): `bytes <= 4` -> `Int32`, otherwise `Int64`
- COMP-1 -> `Float32`; COMP-2 -> `Float64`

COBOL fixed records have no SQL-style NULL, so mapped fields are non-nullable.

## CLI
```
kobold-arrow map <layout.json> [--pretty]
```
Reads a `CobolLayout` JSON and prints the equivalent `ArrowSchema` JSON. Example layout:
```json
{"name":"CUSTOMER-RECORD","fields":[
  {"name":"cust-name","encoding":{"alphanumeric":{"len":30}}},
  {"name":"cust-id","encoding":{"display-numeric":{"digits":5,"scale":0,"signed":false}}},
  {"name":"balance","encoding":{"packed":{"digits":9,"scale":2,"signed":true}}}]}
```

## Library
```rust
use kobold_arrow::{CobolField, CobolLayout, CobolEncoding, map_layout, to_json};

let layout = CobolLayout {
    name: "REC".into(),
    fields: vec![CobolField {
        name: "amount".into(),
        encoding: CobolEncoding::Packed { digits: 7, scale: 2, signed: true },
    }],
};
let schema = map_layout(&layout);          // -> ArrowSchema
let json = to_json(&schema).unwrap();       // Decimal128(precision: 7, scale: 2)
```

## Future work
Building actual Arrow `RecordBatch` columns -- decoding the fixed-record bytes into Arrow arrays on
top of the real `arrow` crate -- is a documented future feature, kept behind an optional dependency
so the schema mapper stays light by default.

## License
Apache-2.0 (see LICENSE).
