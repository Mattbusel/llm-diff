//! Compare two structured (JSON) answers from a model: a readable list of
//! changes for a person, and an RFC 6902 JSON Patch for a program.
//!
//! Run with `cargo run --example structured_output`.

use llm_diff::{json_diff, json_patch, DiffError, JsonDiffOp};

fn main() -> Result<(), DiffError> {
    // Yesterday's and today's answer to "list the three largest French cities".
    let before = r#"{"cities": [
        {"name": "Paris", "population": 2102650},
        {"name": "Marseille", "population": 873076},
        {"name": "Lyon", "population": 522250}
    ], "source": "INSEE 2021"}"#;
    let after = r#"{"cities": [
        {"name": "Paris", "population": 2087577},
        {"name": "Marseille", "population": 873076},
        {"name": "Lyon", "population": 522250},
        {"name": "Toulouse", "population": 504078}
    ], "source": "INSEE 2023", "confidence": "high"}"#;

    println!("What changed:");
    for op in json_diff(before, after)? {
        match op {
            JsonDiffOp::ValueChanged { path, old, new } => println!("  {path}: {old} -> {new}"),
            JsonDiffOp::KeyAdded { path, value } => println!("  + {path} = {value}"),
            JsonDiffOp::KeyRemoved { path, value } => println!("  - {path} (was {value})"),
            JsonDiffOp::Equal => println!("  nothing"),
        }
    }

    let old: serde_json::Value = serde_json::from_str(before)?;
    let new: serde_json::Value = serde_json::from_str(after)?;
    let patch = json_patch(&old, &new);
    println!("\nJSON Patch ({} operations):\n{}", patch.as_array().map_or(0, Vec::len), serde_json::to_string_pretty(&patch)?);
    Ok(())
}
