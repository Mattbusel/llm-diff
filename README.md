# llm-diff

See exactly how an LLM's answer changed when you changed the prompt, the model or the temperature: line, word and character diffs, `git diff`-style patches, JSON diffs and JSON Patches, three-way merges, and a small version history for model outputs, in Rust.

[![crates.io](https://img.shields.io/crates/v/llm-diff.svg)](https://crates.io/crates/llm-diff)
[![docs.rs](https://img.shields.io/docsrs/llm-diff)](https://docs.rs/llm-diff)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://gitlab.com/mattbusel/llm-diff/-/blob/main/LICENSE)

Keep every output you get back, compare any two of them, and record why each one changed. No async runtime, no I/O; it also builds for `wasm32-unknown-unknown`.

## Install

```bash
cargo add llm-diff
# optional: apply patches and three-way merges, Unicode word splitting
cargo add llm-diff --features patch,unicode
```

## In ten lines

```rust
use llm_diff::{json_diff, TextDiff};

let d = TextDiff::compute_words("Paris has 2.1 million people.", "Paris has 2.2 million people.");
let changed: Vec<String> = d.ops.iter().filter(|o| o.kind() != "=").map(|o| format!("{}{}", o.kind(), o.text())).collect();
assert_eq!(changed, ["-2.1", "+2.2"]);
assert!(d.ratio > 0.8 && !d.is_identical());

let ops = json_diff(r#"{"tags":["eu","fr"]}"#, r#"{"tags":["uk","eu","fr"]}"#).unwrap();
assert_eq!(ops.len(), 1); // one element added at $.tags[0], not three changes
```

## Quickstart

```rust
use llm_diff::{json_diff, unified_diff, DiffError, OutputVersion, TextDiff, VersionAnnotation, VersionStore};

fn main() -> Result<(), DiffError> {
    // Model answers are often one paragraph, so diff them word by word.
    let old = "The capital of France is Paris. It sits on the Seine.";
    let new = "The capital of France is Paris. It sits on the river Seine.";
    let diff = TextDiff::compute_words(old, new);
    for op in diff.ops.iter().filter(|op| op.kind() != "=") {
        println!("{} {:?}", op.kind(), op.text());
    }
    println!("ratio {:.2}, identical: {}", diff.ratio, diff.is_identical());

    // A patch you can read like `git diff`.
    print!("{}", unified_diff("a\nb\nc\n", "a\nB\nc\n", 1, "before", "after"));

    // Compare two structured (JSON) outputs field by field.
    for change in json_diff(r#"{"city":"Paris","tags":["eu","fr"]}"#, r#"{"city":"Lyon","tags":["eu"]}"#)? {
        println!("{change:?}"); // ValueChanged at $.city, KeyRemoved at $.tags[1]
    }

    // Keep a history of outputs for one prompt.
    let mut store = VersionStore::new(8_000); // refuse outputs over ~8k tokens
    let v1 = store.store(OutputVersion::new("The answer is Paris.", "gpt-4o", VersionAnnotation::default(), None))?;
    let why = VersionAnnotation { prompt_changed: true, note: Some("added 'be brief'".into()), ..Default::default() };
    let v2 = store.store(OutputVersion::new("Paris.", "gpt-4o", why, Some(v1.clone())))?;
    store.set_branch("main", v2.as_str())?;

    println!("lineage length: {}", store.lineage(&v2)?.len()); // 2
    println!("{}", store.unified_diff(&v1, &v2, 3)?);

    // Save the whole history and load it back later.
    let saved = store.to_json()?;
    let restored = VersionStore::from_json(&saved)?;
    assert_eq!(restored.version_count(), 2);
    Ok(())
}
```

The same program is `examples/quickstart.rs`: `cargo run --example quickstart`.

## Why this and not a diff crate directly

[similar](https://crates.io/crates/similar), [imara-diff](https://crates.io/crates/imara-diff), [diffy](https://crates.io/crates/diffy) and [json-patch](https://crates.io/crates/json-patch) are excellent, and `llm-diff` is built on them. What it adds is the part you would otherwise write yourself when comparing model output:

- one `TextDiff` type for line, word, Unicode-word and character diffs, with owned ops you can store or serialize, an `is_identical` that means byte-for-byte equal, and two scores (order-aware `ratio` and word-overlap `similarity`);
- a JSON diff that aligns arrays (an element inserted at the front is one change, where json-patch's own `diff` rewrites every later index), plus the same result as an RFC 6902 patch;
- a version store with content addresses, branches, lineage, "why it changed" notes, three-way merge and JSON save/load.

If all you need is the fastest possible line diff, call imara-diff directly: see Performance below.

## Already using another crate?

- **similar**: `llm_diff::similar` is a re-export of the version used here, for features not wrapped (inline highlighting, custom algorithms, byte diffs).
- **json-patch** (or `fast-json-patch` in JS, `jsonpatch` in Python): `llm_diff::json_patch(&old, &new)` returns RFC 6902 JSON; deserialize it into `json_patch::Patch` and apply it. A property test applies 400 random patches with the json-patch crate.
- **diffy / git / `patch`**: `unified_diff` output applies with `apply_patch` (feature `patch`, uses diffy), with `git apply` and with `patch`. A property test round-trips 400 random patches through diffy.
- **serde**: every result type derives `Serialize`/`Deserialize`; with the `schemars` feature they also derive `JsonSchema`.

## Features

| Feature | Default | What it adds | Extra dependencies |
|---|---|---|---|
| (none) | yes | line/word/char diffs, unified diffs, JSON diff and JSON Patch, version store, audit log | similar, imara-diff, sha2, serde_json, uuid, chrono, thiserror |
| `patch` | no | `apply_patch` (apply a unified diff), `merge3` and `VersionStore::merge` (three-way merge with git-style conflict markers) | [diffy](https://crates.io/crates/diffy) |
| `unicode` | no | `TextDiff::compute_unicode_words`: word boundaries from Unicode (UAX #29), for Chinese, Japanese, Thai and punctuation next to words | similar's `unicode` feature (unicode-segmentation) |
| `schemars` | no | JSON Schema for the serialized types | [schemars](https://crates.io/crates/schemars) |

## Examples

| Example | Shows | Run |
|---|---|---|
| `quickstart` | all the basics | `cargo run --example quickstart` |
| `prompt_regression` | flag answers that moved a lot after a prompt change; word, char and line diffs | `cargo run --example prompt_regression` |
| `structured_output` | readable JSON changes and an RFC 6902 patch for the same pair of answers | `cargo run --example structured_output` |
| `branch_merge` | two branches edit one answer, three-way merge, a conflict, a patch | `cargo run --example branch_merge --features patch` |

## What you get

| API | What it does |
|---|---|
| `TextDiff::compute(old, new)` | Line diff (Histogram algorithm from imara-diff, git-style alignment) |
| `TextDiff::compute_words(old, new)` | Word diff on whitespace, for single-paragraph answers |
| `TextDiff::compute_unicode_words` | Word diff on Unicode word boundaries (feature `unicode`) |
| `TextDiff::compute_chars(old, new)` | Character diff, for short strings and numbers |
| `diff.ratio` | Order-aware similarity: 2 x matched / total elements, 0.0 to 1.0 |
| `diff.similarity` | Word overlap (Jaccard), ignores word order |
| `diff.is_identical()` | True only for byte-for-byte equal texts |
| `unified_diff(old, new, context, old_name, new_name)` | `diff -u` / `git diff` style patch |
| `apply_patch`, `merge3` | apply a patch; three-way merge (feature `patch`) |
| `json_diff(old, new)` | Field-by-field JSON diff with paths like `$.items[2].price`; arrays aligned |
| `json_patch(&old, &new)` | The same as an RFC 6902 JSON Patch |
| `VersionStore` | Versions with SHA-256 content addresses, model, time, parent link and a "why it changed" note; branches, `lineage`, `rollback`, `diff_versions`, `unified_diff`, `merge`, JSON save and load |
| `AuditLog` | Append-only log of version, branch, rollback and diff events, serializable to JSON |

## Performance

`benches/vs_alternatives.rs`, same generated inputs for every crate. Intel i7-13700KF, Windows 11, release build, other work on the machine (rough numbers):

| Line diff | 300-line answer, 15 lines edited | 5,000-line document, 50 lines edited |
|---|---|---|
| llm-diff 0.2 `TextDiff::compute` | 143 µs | 2.8 ms |
| llm-diff 0.1.1 (hand-written LCS) | 411 µs | 129 ms |
| similar 3 `from_lines` (ops only) | 85 µs | 1.7 ms |
| imara-diff 0.2 Histogram (counts only) | 18 µs | 0.60 ms |
| diffy 0.5 `create_patch` | 45 µs | 0.73 ms |

| Word diff, one 2,000-word paragraph, 80 words changed | |
|---|---|
| llm-diff 0.2 `compute_words` | 1.10 ms |
| similar 3 `from_words` | 0.87 ms |
| dissimilar 1 (character based, semantic cleanup) | 2.53 ms |

The line diff itself is imara-diff; most of llm-diff's extra time goes to the word-overlap score (hashing every word of both texts) and to copying each line into an owned op. 0.1.1 was 47 times slower on the 5,000-line case and needed a 200 MB table for it. Reproduce with `cargo bench --bench vs_alternatives`.

## Limitations

- The token limit in `VersionStore::new` is an estimate (one token per four bytes).
- `apply_patch` and `merge3` work line by line, like git; edits on directly adjacent lines conflict.
- Histogram line diffs are not always the shortest possible edit script; they are the ones git shows with `--histogram`.
- The audit log is not written automatically by the store; record events yourself.

Run the tests with `cargo test --all-features`. Contributions are welcome, see [CONTRIBUTING.md](https://gitlab.com/mattbusel/llm-diff/-/blob/main/CONTRIBUTING.md).

## License

MIT, see [LICENSE](https://gitlab.com/mattbusel/llm-diff/-/blob/main/LICENSE).
