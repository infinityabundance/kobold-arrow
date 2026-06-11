//! `kobold-arrow` CLI: map a COBOL fixed-record layout (JSON) to the equivalent Apache Arrow
//! schema and print it as JSON.
//!
//! Usage:
//!   kobold-arrow map <layout.json> [--pretty]
//!
//! The layout JSON matches [`kobold_arrow::CobolLayout`], e.g.:
//!   {"name":"CUSTOMER-RECORD","fields":[
//!      {"name":"cust-name","encoding":{"alphanumeric":{"len":30}}},
//!      {"name":"balance","encoding":{"packed":{"digits":9,"scale":2,"signed":true}}}]}
#![forbid(unsafe_code)]

use kobold_arrow::{map_layout, to_json, to_json_pretty, CobolLayout};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 || args[1] != "map" {
        eprintln!("usage: kobold-arrow map <layout.json> [--pretty]");
        return ExitCode::from(2);
    }

    let mut layout_path: Option<String> = None;
    let mut pretty = false;
    for arg in &args[2..] {
        match arg.as_str() {
            "--pretty" => pretty = true,
            other => layout_path = Some(other.to_string()),
        }
    }

    let Some(lp) = layout_path else {
        eprintln!("error: need a <layout.json> path");
        return ExitCode::from(2);
    };

    let src = match std::fs::read_to_string(&lp) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read layout {lp}: {e}");
            return ExitCode::from(2);
        }
    };
    let layout: CobolLayout = match serde_json::from_str(&src) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: invalid layout JSON: {e}");
            return ExitCode::from(2);
        }
    };

    let schema = map_layout(&layout);
    let json = if pretty { to_json_pretty(&schema) } else { to_json(&schema) };
    match json {
        Ok(s) => {
            println!("{s}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: serialize schema: {e}");
            ExitCode::from(2)
        }
    }
}
