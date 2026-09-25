# llm-diff

[![CI](https://github.com/Mattbusel/llm-diff/actions/workflows/ci.yml/badge.svg)](https://github.com/Mattbusel/llm-diff/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/llm-diff.svg)](https://crates.io/crates/llm-diff)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Diff, version and audit LLM outputs in Rust: line diffs with a similarity score, structural JSON diffs, and a content-addressed version store with branches and lineage.

When you change a prompt, a model or a temperature, the output changes too, and you usually want to know exactly how. `llm-diff` is a small, dependency-light library for keeping every output version, comparing any two of them, and recording who changed what for later review.

## Features

- **`TextDiff::compute(old, new)`**: line-level diff (LCS) as a list of `DiffOp::Equal / Insert / Delete`, plus a word-level Jaccard `similarity` score from 0.0 to 1.0.
- **`json_diff(old, new)`**: structural diff of two JSON documents, reporting `ValueChanged`, `KeyAdded` and `KeyRemoved` with their JSON paths. Useful for structured-output regressions.
- **`VersionStore`**: in-memory store of `OutputVersion`s, each with a UUID, an FNV-1a content address, the model name, a timestamp, a parent link and a `VersionAnnotation` (prompt / model / temperature changed, free-form note).
  - named branches (`set_branch`, `branch_head`)
  - `diff_versions(from, to)`, `rollback(id)` (returns the parent), `lineage(id)` (full ancestor chain)
  - a per-version size cap (estimated tokens) that rejects oversized outputs
- **`AuditLog`**: append-only log of `AuditEvent`s (version created, branch created, rollback, diff computed) that serializes to JSON.

## Quick start

```bash
cargo add llm-diff
```

```rust
use llm_diff::{json_diff, TextDiff, OutputVersion, VersionAnnotation, VersionStore, DiffError};

fn main() -> Result<(), DiffError> {
    // Compare two answers.
    let diff = TextDiff::compute(
        "The capital of France is Paris.\nIt is on the Seine.",
        "The capital of France is Lyon.\nIt is on the Seine.",
    );
    println!("+{} -{} similarity {:.2}", diff.insertions(), diff.deletions(), diff.similarity);
    for op in &diff.ops {
        println!("{} {}", op.kind(), op.text());
    }

    // Compare two structured outputs.
    for change in json_diff(r#"{"city":"Paris","pop":2.1}"#, r#"{"city":"Lyon","pop":2.1}"#)? {
        println!("{change:?}");
    }

    // Keep a history of outputs for one prompt.
    let mut store = VersionStore::new(8_000); // max ~8k tokens per version
    let v1 = store.store(OutputVersion::new("The answer is Paris.", "gpt-4o", VersionAnnotation::default(), None))?;
    let note = VersionAnnotation { prompt_changed: true, note: Some("added 'be brief'".into()), ..Default::default() };
    let v2 = store.store(OutputVersion::new("The answer is Paris, France.", "gpt-4o", note, Some(v1.clone())))?;

    store.set_branch("main", v2.as_str())?;
    let changes = store.diff_versions(&v1, &v2)?;
    println!("similarity v1 -> v2: {:.2}", changes.similarity);
    println!("lineage length: {}", store.lineage(&v2)?.len()); // 2
    Ok(())
}
```

## How it works

| File | What it holds |
|---|---|
| `src/diff.rs` | `TextDiff`, `DiffOp`, LCS line diff, Jaccard similarity, `json_diff` / `JsonDiffOp` |
| `src/store.rs` | `VersionStore`, `OutputVersion`, `VersionAnnotation`, FNV-1a content addressing |
| `src/audit.rs` | `AuditLog`, `AuditEvent` |
| `src/error.rs` | `DiffError` |

The line diff is a classic dynamic-programming LCS, so it is O(n * m) in lines: fine for LLM responses, not meant for large files.

## Status

Version 0.1. The store is in-memory only; serialize `OutputVersion` (it derives `Serialize`) if you need persistence. The audit log is not written automatically by the store; record events yourself.

```bash
cargo test
cargo bench
```

## License

MIT, see [LICENSE](LICENSE).

---

Part of a set of Rust crates for LLM agents, see [rust-crates](https://github.com/Mattbusel/rust-crates).
