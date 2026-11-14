// SPDX-License-Identifier: MIT
//! Compare LLM outputs: line, word and character diffs, `git diff`-style
//! unified patches, structural JSON diffs and RFC 6902 JSON Patches, plus an
//! optional content-addressed history of outputs.
//!
//! No async runtime and no I/O. Builds for `wasm32-unknown-unknown`.
//!
//! # Features
//!
//! | Feature | Default | Adds | Extra dependencies |
//! |---|---|---|---|
//! | (core) | always | [`TextDiff`], [`unified_diff`], [`json_diff`], [`json_diff_values`], [`json_patch`] | similar, imara-diff, foldhash, serde, serde_json, thiserror |
//! | `store` | yes | [`VersionStore`], [`OutputVersion`], [`content_address`], [`AuditLog`] | uuid, chrono, sha2 |
//! | `patch` | no | `apply_patch`, `merge3`, `VersionStore::merge` | diffy |
//! | `unicode` | no | `TextDiff::compute_unicode_words` (UAX #29 word boundaries for CJK, Thai, punctuation) | unicode-segmentation (via similar) |
//! | `schemars` | no | `JsonSchema` for every serialized type | schemars |
//!
//! For diffs only, turn the store off:
//!
//! ```toml
//! llm-diff = { version = "0.3", default-features = false }
//! ```
//!
//! # Which diff do I use?
//!
//! | You have | Call | You get |
//! |---|---|---|
//! | Two answers that are one paragraph each | [`TextDiff::compute_words`] | word and whitespace ops; `ratio()` near 1.0 means a small edit |
//! | Multi-line text (code, lists, documents) | [`TextDiff::compute`] | one op per line, Histogram algorithm as in `git diff --histogram` |
//! | The same, as a patch for a person or for `git apply` | [`unified_diff`] | `diff -u` text, same changed lines as [`TextDiff::compute`] |
//! | A short string such as a number or a name | [`TextDiff::compute_chars`] | one op per character |
//! | Chinese, Japanese or Thai text | `TextDiff::compute_unicode_words` (feature `unicode`) | word ops on Unicode word boundaries |
//! | Two JSON (structured output) answers, for a person | [`json_diff`] / [`json_diff_values`] | changes with paths like `$.items[1].price`; arrays are aligned, so one inserted element is one change |
//! | The same, for a program | [`json_patch`] | RFC 6902 JSON Patch any JSON Patch library can apply |
//! | A history of answers to one prompt | [`VersionStore`] (feature `store`) | deduplicated versions, branches, lineage, diffs between any two, JSON save and load |
//! | Two edits of one answer to combine | `merge3` / `VersionStore::merge` (feature `patch`) | three-way merge with git-style conflict markers |
//!
//! # Text diffs
//!
//! ```
//! use llm_diff::TextDiff;
//!
//! let d = TextDiff::compute_words("Paris has 2.1 million people.", "Paris has 2.2 million people.");
//! let changed: Vec<String> = d.changes().map(|op| format!("{}{}", op.kind(), op.text())).collect();
//! assert_eq!(changed, ["-2.1", "+2.2"]);
//! assert!(d.ratio() > 0.8 && !d.is_identical());
//!
//! let lines = TextDiff::compute("a\nb\nc\n", "a\nB\nc\n");
//! assert_eq!((lines.deletions(), lines.insertions()), (1, 1));
//!
//! let patch = llm_diff::unified_diff("a\nb\nc\n", "a\nB\nc\n", 1, "before", "after");
//! assert_eq!(patch, "--- before\n+++ after\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n");
//! ```
//!
//! # JSON diffs
//!
//! ```
//! use llm_diff::{json_diff, json_patch, JsonDiffOp};
//! use serde_json::json;
//!
//! let ops = json_diff(r#"{"tags":["eu","fr"]}"#, r#"{"tags":["uk","eu","fr"]}"#)?;
//! assert_eq!(ops, [JsonDiffOp::KeyAdded { path: "$.tags[0]".into(), value: json!("uk") }]);
//!
//! let patch = json_patch(&json!({"city": "Paris"}), &json!({"city": "Lyon"}));
//! assert_eq!(patch, json!([{"op": "replace", "path": "/city", "value": "Lyon"}]));
//! # Ok::<(), llm_diff::DiffError>(())
//! ```
//!
//! # Version store (feature `store`)
//!
//! ```
//! # #[cfg(feature = "store")] {
//! use llm_diff::{OutputVersion, VersionAnnotation, VersionStore};
//!
//! let mut store = VersionStore::new(64 * 1024); // refuse outputs over 64 KiB
//! let v1 = store.store(OutputVersion::new("The answer is Paris.", "gpt-4o", VersionAnnotation::default(), None))?;
//! let why = VersionAnnotation { prompt_changed: true, ..Default::default() };
//! let v2 = store.store(OutputVersion::new("Paris.", "gpt-4o", why, Some(v1.clone())))?;
//! assert!(store.unified_diff(&v1, &v2, 3)?.contains("+Paris."));
//! # }
//! # Ok::<(), llm_diff::DiffError>(())
//! ```
//!
//! # Stability and MSRV
//!
//! The minimum supported Rust version (`rust-version` in Cargo.toml) is 1.85,
//! and CI builds every feature on 1.85. Raising it is a minor-version change.
//! The diff-only core (`default-features = false`) builds on 1.85 with the
//! newest versions of its dependencies. The `store` feature does not: the
//! latest uuid and sha2 releases need a newer Rust, so on 1.85 resolve with
//! `CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback` (or
//! `resolver = "3"` / `package.resolver.incompatible-rust-versions = "fallback"`)
//! so Cargo picks releases that still support 1.85. Result types ([`TextDiff`], [`DiffOp`],
//! [`JsonDiffOp`], [`DiffError`], `AuditEvent`) are `#[non_exhaustive]`, so
//! new fields and variants can arrive in minor releases: use the accessor
//! methods and add a wildcard arm when you match. The crates this one is
//! built on (similar, imara-diff, diffy) are not part of its public API.
//!
//! Examples live in `examples/`: `quickstart`, `prompt_regression`,
//! `structured_output` and `branch_merge` (`--features patch`).
#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]
// The crate docs link to the `store` items; without that feature they are plain text.
#![cfg_attr(not(feature = "store"), allow(rustdoc::broken_intra_doc_links))]

#[cfg(feature = "store")]
#[cfg_attr(docsrs, doc(cfg(feature = "store")))]
pub mod audit;
pub mod diff;
/// The error type.
pub mod error;
#[cfg(feature = "store")]
#[cfg_attr(docsrs, doc(cfg(feature = "store")))]
pub mod store;

#[cfg(feature = "store")]
pub use audit::{AuditEvent, AuditLog};
pub use diff::{json_diff, json_diff_values, json_patch, unified_diff, DiffOp, JsonDiffOp, TextDiff};
#[cfg(feature = "patch")]
pub use diff::{apply_patch, merge3};
pub use error::DiffError;
#[cfg(feature = "store")]
pub use store::{content_address, OutputVersion, VersionAnnotation, VersionStore};

// The README's code blocks run as doctests, without making the README the
// crate docs.
#[cfg(all(doctest, feature = "store"))]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
