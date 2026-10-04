# Changelog

## 0.2.0 (2026-10-03)

### Added
- Line diffs use imara-diff's Histogram algorithm (as in `git diff --histogram`) with git-style slider alignment. On a 5,000-line document this is 2.8 ms against 129 ms for 0.1.1 (measured, see README).
- `TextDiff::compute_chars` (character diff) and, with the `unicode` feature, `TextDiff::compute_unicode_words` (Unicode word boundaries for text without spaces, such as Chinese).
- `json_patch(&old, &new)`: the JSON diff as an RFC 6902 JSON Patch, ordered so it applies sequentially. Property-tested by applying 400 random patches with the json-patch crate.
- Feature `patch` (diffy): `apply_patch` applies unified diffs (rejects text with no hunks instead of silently doing nothing, which is what diffy alone does), `merge3` and `VersionStore::merge` do three-way merges with git-style conflict markers. Our unified diffs were also checked with `git apply` and `patch -p1`.
- Feature `schemars`: JSON Schema for the serialized types.
- `pub use similar` for features not wrapped here; `DiffError` is `#[non_exhaustive]` and has a `MergeConflict` variant.
- Property tests (ops rebuild both texts for line, word and char diffs; JSON diff finds exactly the changes; random strings never panic), `benches/vs_alternatives.rs` against similar, imara-diff, diffy, dissimilar and 0.1.1, three more examples (`prompt_regression`, `structured_output`, `branch_merge`), CONTRIBUTING.md, issue templates, an MSRV job in CI.
- `TextDiff::compute_words`: word-level diff, for model answers that are one long paragraph (a line diff just shows the whole paragraph replaced).
- `unified_diff(old, new, context, old_name, new_name)` and `VersionStore::unified_diff`: `diff -u` / `git diff` style patches.
- `TextDiff::ratio`: order-aware similarity from the diff.
- `json_diff_values` for already-parsed JSON.
- `VersionStore` now derives `Serialize`/`Deserialize`, with `to_json` / `from_json`, so a history can be saved and loaded.
- `examples/quickstart.rs`; the README is the crate docs and its quickstart is a doctest.

### Fixed
- The old hand-written LCS line diff built an (n+1) x (m+1) table: about 200 MB for two 5,000-line texts and about 3.2 GB for 20,000 lines. Line diffs now need memory proportional to the input.
- `is_identical()` was decided from the word-set similarity, so `"yes no"` and `"no yes"` were reported as identical. It is now true only for equal texts.
- A missing or extra newline at the end of the text was invisible to the diff. It now shows as a changed line. Windows line endings are stripped from line ops.
- Identical inputs returned the whole text as one op instead of one op per line.
- `json_diff` reported a changed array as one big changed value; it now aligns arrays with a Myers diff of their elements, so inserting one element at the front is one `KeyAdded` instead of every later element changing, and recurses into edited elements (`$.items[1].price`). Keys containing dots or spaces are written as `$["first name"]` so paths are unambiguous.
- Content addresses are SHA-256 (via `sha2`) instead of 64-bit FNV-1a, which is not collision resistant. Addresses stored by 0.1.x will not match.
- `VersionStore::store` accepted a parent id that did not exist, and `lineage` failed later. It now returns `VersionNotFound` up front.
- Builds for `wasm32-unknown-unknown` (uuid's `js` feature is enabled for that target; 0.1.x failed to compile there).

### Changed
- `TextDiff` has two new public fields (`ratio`, `identical`, both `#[serde(default)]`).
- `JsonDiffOp` derives `PartialEq`.
- MSRV is 1.85 (required by `similar` 3), checked in CI.

## 0.1.1

- First published version.
