# Changelog

## 0.3.2 (2026-11-20)

- README opens with the side this crate takes: a similarity score is how a changed number passes your eval. Why it exists, why it beats the usual approach, why adopt it today, who it is for and the tradeoffs made on purpose.
- crates.io description and keywords say the same thing.

## [0.3.1] - 2026-11-16

- Package metadata only: `repository` points at https://github.com/Mattbusel/llm-diff (the GitHub repository, where issues, releases and the dependents graph are), no code change from 0.3.0.

## 0.3.0 (2026-11-14, not yet published)

A diff-only core, a version store that actually deduplicates, and types that
can grow without breaking you. Breaking release: see "Upgrading from 0.2".

### Added
- Feature `store` (on by default) holds `VersionStore`, `OutputVersion`, `content_address`, `AuditLog` and their errors, and owns the uuid, chrono and sha2 dependencies. `llm-diff = { version = "0.3", default-features = false }` is a diff-only build: similar, imara-diff, foldhash, serde, serde_json and thiserror (17 unique crates in `cargo tree -e normal` instead of 31 with the store, and no uuid, getrandom, chrono or sha2).
- `TextDiff` accessors: `ops()`, `changes()` (only inserts and deletes), `into_ops()`, `ratio()`, `similarity()`. `DiffOp::is_change()`.
- `VersionStore::max_bytes()`, `VersionStore::iter()`, `VersionStore::branches()`.
- `VersionAnnotation::metadata`, a `BTreeMap<String, serde_json::Value>` for anything else worth keeping (seed, prompt id, eval score). Left out of the JSON when empty, so 0.2 annotations still load.
- `DiffError::DuplicateVersionId` and `DiffError::InvalidStore`.
- Real crate docs on docs.rs: a feature table, a "which diff do I use" table, one example per module, stability and MSRV notes. The README is no longer the crate docs, but its code blocks still run as doctests.
- Property tests: every `-`/`+` line of `unified_diff` is exactly a `Delete`/`Insert` op of `TextDiff::compute`; `json_diff` and `json_patch` agree, and the patch applied with the json-patch crate gives the new value.
- CI: a `cargo hack test --each-feature` job, `--no-default-features` tests and wasm32 build, and an MSRV build of the diff-only core (GitLab); the same feature, docs and MSRV jobs in the GitHub workflow.

### Changed
- **`VersionStore::store` deduplicates.** Storing text that is already in the store returns the existing version's id and adds nothing (0.2 documented deduplication but stored a second copy and silently repointed the address index). The content address is recomputed from the text, so a version whose `content` was edited after `OutputVersion::new` is still filed correctly.
- **The size limit is in bytes.** `VersionStore::new(max_bytes)` replaces `new(max_output_tokens)`, where tokens were guessed as bytes / 4, which is badly wrong for CJK text. `DiffError::OutputTooLarge { size, limit }` is in bytes. Count tokens with your model's tokenizer before `store` if you need a token limit. Stores saved by 0.2 still load; their `max_output_tokens` number is read as bytes.
- **`VersionStore::from_json` validates** what it loads: every version is filed under its own id, parents exist, and branches and address entries point at stored versions. Otherwise it returns `DiffError::InvalidStore`. Every `content_address` is recomputed from the text, so a 0.2 save holding a version whose `content` was edited after `OutputVersion::new` (0.2 kept the stale address) still loads, re-filed under the right address. Stale address entries are dropped and missing ones rebuilt.
- **`unified_diff` uses the same Histogram line diff as `TextDiff::compute`** (it used similar's Myers diff), so the patch and the ops always show the same changed lines. The output format is unchanged, including `\ No newline at end of file`.
- `TextDiff`, `DiffOp`, `JsonDiffOp` and `AuditEvent` are `#[non_exhaustive]`, like `DiffError` already was. Fields of `TextDiff` can still be read.
- thiserror 1 -> 2. `DiffError` messages start with a lowercase letter.
- `AuditLog` derives `Debug` and `Clone`; `AuditEvent` and `VersionAnnotation` derive `PartialEq`.

### Removed
- `pub use similar`. similar's version was part of this crate's API, so every similar major release would have been a breaking llm-diff release. Depend on `similar` directly if you need it.
- `DiffError::RollbackFailed` and `DiffError::ContentAddressCollision`, which nothing ever returned.

### Upgrading from 0.2
- `llm_diff::similar::...` -> add `similar = "3"` to your Cargo.toml and use `similar::...`.
- `match` on `DiffOp`, `JsonDiffOp`, `AuditEvent` or `DiffError` needs a `_ =>` arm.
- `VersionStore::new(n)`: `n` is now bytes. Multiply a 0.2 token limit by about 4 to keep the same behavior for English text.
- Code that stored the same text twice and expected two ids now gets the same id back.
- With `default-features = false`, add `features = ["store"]` if you use the version store or audit log.

## 0.2.1 (2026-10-21)

- Published from the GitLab repository with a new README. No code changes.

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
