// SPDX-License-Identifier: MIT
//! Diff primitives: line and word diffs of text, unified diff output, and a
//! structural diff of JSON documents.
//!
//! Line diffs use [imara-diff](https://crates.io/crates/imara-diff)
//! (Histogram algorithm, as in `git diff --histogram`); word and character
//! diffs and unified patches use [similar](https://crates.io/crates/similar)
//! (Myers). Both need memory proportional to the size of the input rather than
//! to the product of the two sizes.

use crate::error::DiffError;
use serde::{Deserialize, Serialize};

/// A single edit operation in a text diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum DiffOp {
    /// Content present in both old and new.
    Equal(String),
    /// Content only in the new text.
    Insert(String),
    /// Content only in the old text.
    Delete(String),
}

impl DiffOp {
    /// Returns a single-character label: `=`, `+`, or `-`.
    pub fn kind(&self) -> &'static str {
        match self {
            DiffOp::Equal(_) => "=",
            DiffOp::Insert(_) => "+",
            DiffOp::Delete(_) => "-",
        }
    }

    /// Returns the text content of the operation.
    pub fn text(&self) -> &str {
        match self {
            DiffOp::Equal(s) | DiffOp::Insert(s) | DiffOp::Delete(s) => s,
        }
    }
}

/// The result of comparing two text outputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TextDiff {
    /// Edit operations that turn the old text into the new one. For a line
    /// diff each op is one line without its line ending; for a word diff each
    /// op is one word or one run of whitespace.
    pub ops: Vec<DiffOp>,
    /// Word overlap (Jaccard index of the two sets of words): 0.0 = no word in
    /// common, 1.0 = the same words. Ignores order and repetition, so
    /// `"yes no"` and `"no yes"` score 1.0. Use [`ratio`](Self::ratio) for an
    /// order-aware score.
    pub similarity: f64,
    /// Order-aware similarity from the diff itself: 2 * matched / total
    /// elements (lines or words), between 0.0 and 1.0.
    #[serde(default)]
    pub ratio: f64,
    /// True only when the two texts were byte-for-byte equal.
    #[serde(default)]
    pub identical: bool,
}

fn strip_line_ending(s: &str) -> &str {
    let s = s.strip_suffix('\n').unwrap_or(s);
    s.strip_suffix('\r').unwrap_or(s)
}

fn ops_from(diff: &similar::TextDiff<'_, '_, str>, strip_endings: bool) -> Vec<DiffOp> {
    diff.iter_all_changes()
        .map(|c| {
            let v = c.value();
            let text = if strip_endings { strip_line_ending(v) } else { v }.to_string();
            match c.tag() {
                similar::ChangeTag::Equal => DiffOp::Equal(text),
                similar::ChangeTag::Insert => DiffOp::Insert(text),
                similar::ChangeTag::Delete => DiffOp::Delete(text),
            }
        })
        .collect()
}

/// Line ops and the order-aware ratio, via imara-diff's Histogram algorithm.
fn line_ops(old: &str, new: &str) -> (Vec<DiffOp>, f64) {
    use imara_diff::{Algorithm, Diff, InternedInput};
    let input = InternedInput::new(old, new);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let line = |t: imara_diff::Token| strip_line_ending(input.interner[t]).to_string();
    let mut ops = Vec::with_capacity(input.before.len().max(input.after.len()));
    let mut equal = 0usize;
    let mut b = 0usize; // next unconsumed line in `before`
    for hunk in diff.hunks() {
        let (bs, be) = (hunk.before.start as usize, hunk.before.end as usize);
        let (as_, ae) = (hunk.after.start as usize, hunk.after.end as usize);
        for &t in &input.before[b..bs] {
            ops.push(DiffOp::Equal(line(t)));
            equal += 1;
        }
        ops.extend(input.before[bs..be].iter().map(|&t| DiffOp::Delete(line(t))));
        ops.extend(input.after[as_..ae].iter().map(|&t| DiffOp::Insert(line(t))));
        b = be;
    }
    for &t in &input.before[b..] {
        ops.push(DiffOp::Equal(line(t)));
        equal += 1;
    }
    let total = input.before.len() + input.after.len();
    let ratio = if total == 0 { 1.0 } else { 2.0 * equal as f64 / total as f64 };
    (ops, ratio)
}

impl TextDiff {
    /// Line-level diff between `old` and `new`.
    ///
    /// A missing or extra newline at the end of the text counts as a change to
    /// the last line.
    ///
    /// Uses the Histogram algorithm from
    /// [imara-diff](https://crates.io/crates/imara-diff) (the algorithm git
    /// offers as `--histogram`), with git's slider post-processing, which
    /// aligns changes on blank lines and indentation the way people expect.
    pub fn compute(old: &str, new: &str) -> Self {
        let (ops, ratio) = line_ops(old, new);
        Self { ops, similarity: compute_similarity(old, new), ratio, identical: old == new }
    }

    /// Word-level diff between `old` and `new`. Better than a line diff for
    /// model answers, which are often one long paragraph.
    ///
    /// Ops alternate between words and the whitespace between them, so
    /// concatenating the `Equal` and `Insert` texts gives back `new` exactly.
    pub fn compute_words(old: &str, new: &str) -> Self {
        let d = similar::TextDiff::from_words(old, new);
        Self {
            ops: ops_from(&d, false),
            similarity: compute_similarity(old, new),
            ratio: f64::from(d.ratio()),
            identical: old == new,
        }
    }

    /// Character-level diff (one op per Unicode scalar value). Useful for
    /// short strings such as labels, names or numbers inside an answer.
    pub fn compute_chars(old: &str, new: &str) -> Self {
        let d = similar::TextDiff::from_chars(old, new);
        Self {
            ops: ops_from(&d, false),
            similarity: compute_similarity(old, new),
            ratio: f64::from(d.ratio()),
            identical: old == new,
        }
    }

    /// Word-level diff using Unicode word boundaries (UAX #29) instead of
    /// whitespace, so text without spaces (Chinese, Japanese, Thai) and
    /// punctuation next to words are split properly. Needs the `unicode` feature.
    #[cfg(feature = "unicode")]
    #[cfg_attr(docsrs, doc(cfg(feature = "unicode")))]
    pub fn compute_unicode_words(old: &str, new: &str) -> Self {
        let d = similar::TextDiff::from_unicode_words(old, new);
        Self {
            ops: ops_from(&d, false),
            similarity: compute_similarity(old, new),
            ratio: f64::from(d.ratio()),
            identical: old == new,
        }
    }

    /// Returns the number of inserted lines (or words for [`compute_words`](Self::compute_words)).
    pub fn insertions(&self) -> usize {
        self.ops.iter().filter(|op| matches!(op, DiffOp::Insert(_))).count()
    }

    /// Returns the number of deleted lines (or words).
    pub fn deletions(&self) -> usize {
        self.ops.iter().filter(|op| matches!(op, DiffOp::Delete(_))).count()
    }

    /// Returns `true` if the two texts were exactly the same.
    pub fn is_identical(&self) -> bool {
        self.identical
    }
}

/// A unified diff (the format of `diff -u` and `git diff`) of two texts, with
/// `context` unchanged lines around each change. Returns an empty string when
/// the texts are equal.
///
/// ```
/// let patch = llm_diff::unified_diff("a\nb\nc\n", "a\nB\nc\n", 1, "v1", "v2");
/// assert_eq!(patch, "--- v1\n+++ v2\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n");
/// ```
pub fn unified_diff(old: &str, new: &str, context: usize, old_name: &str, new_name: &str) -> String {
    if old == new {
        return String::new();
    }
    similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(context)
        .header(old_name, new_name)
        .to_string()
}

/// Apply a unified diff (as made by [`unified_diff`], `diff -u` or
/// `git diff` for one file) to `original`. Needs the `patch` feature; parsing
/// and applying are done by [diffy](https://crates.io/crates/diffy).
///
/// An empty patch returns `original` unchanged; any other text without a
/// `@@` hunk is rejected rather than silently ignored.
///
/// # Errors
/// [`DiffError::InvalidDiff`] if the patch does not parse, has no hunks, or
/// its context lines do not match `original`.
#[cfg(feature = "patch")]
#[cfg_attr(docsrs, doc(cfg(feature = "patch")))]
pub fn apply_patch(original: &str, patch: &str) -> Result<String, DiffError> {
    if patch.is_empty() {
        return Ok(original.to_string());
    }
    let parsed = diffy::Patch::from_str(patch).map_err(|e| DiffError::InvalidDiff(format!("patch does not parse: {e}")))?;
    // diffy reads text with no hunks as an empty patch and would silently
    // return the original unchanged; treat that as an error instead.
    if parsed.hunks().is_empty() && !patch.trim().is_empty() {
        return Err(DiffError::InvalidDiff("no hunks (@@ lines) found in the patch".into()));
    }
    diffy::apply(original, &parsed).map_err(|e| DiffError::InvalidDiff(format!("patch does not apply: {e}")))
}

/// Three-way merge: combine the edits `ours` and `theirs` each made to
/// `ancestor`. Returns the merged text, or [`DiffError::MergeConflict`]
/// holding the text with git-style `<<<<<<<` / `>>>>>>>` conflict markers
/// when both changed the same lines differently. As in git, edits to
/// directly adjacent lines also count as a conflict. Needs the `patch` feature
/// (uses [diffy](https://crates.io/crates/diffy)).
///
/// # Errors
/// [`DiffError::MergeConflict`] when the edits overlap.
#[cfg(feature = "patch")]
#[cfg_attr(docsrs, doc(cfg(feature = "patch")))]
pub fn merge3(ancestor: &str, ours: &str, theirs: &str) -> Result<String, DiffError> {
    diffy::merge(ancestor, ours, theirs).map_err(DiffError::MergeConflict)
}

/// Computes Jaccard similarity on word sets.
fn compute_similarity(a: &str, b: &str) -> f64 {
    // foldhash (already used by imara-diff) is several times faster than the
    // default SipHash here, and |A u B| = |A| + |B| - |A n B| avoids a third set.
    type Set<'a> = std::collections::HashSet<&'a str, foldhash::fast::FixedState>;
    let words_a: Set<'_> = a.split_whitespace().collect();
    let words_b: Set<'_> = b.split_whitespace().collect();
    let (small, large) = if words_a.len() <= words_b.len() { (&words_a, &words_b) } else { (&words_b, &words_a) };
    let intersection = small.iter().filter(|w| large.contains(*w)).count();
    let union = words_a.len() + words_b.len() - intersection;
    if union == 0 { 1.0 } else { intersection as f64 / union as f64 }
}

/// An operation in a structural JSON diff.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum JsonDiffOp {
    /// A value changed at the given JSON path (also used when the type changed).
    ValueChanged {
        /// Where, for example `$.items[2].name`.
        path: String,
        /// Value before.
        old: serde_json::Value,
        /// Value after.
        new: serde_json::Value,
    },
    /// A key was added to an object, or an element appended to an array.
    KeyAdded {
        /// Where the new value is.
        path: String,
        /// The new value.
        value: serde_json::Value,
    },
    /// A key was removed from an object, or an element dropped from the end of an array.
    KeyRemoved {
        /// Where the value was.
        path: String,
        /// The removed value.
        value: serde_json::Value,
    },
    /// No differences found.
    Equal,
}

/// Computes a structural diff between two JSON strings.
///
/// Objects are compared key by key. Arrays are aligned with a Myers diff of
/// their elements, so inserting one element at the front reports one
/// `KeyAdded`, not every later element as changed. Elements that were edited
/// in place are compared recursively, so a change deep inside an array is
/// reported at its own path, for example `$.items[1].price`.
///
/// Paths of removed elements use their index in the old array; paths of added
/// and changed elements use their index in the new array. Keys that are not
/// plain identifiers are written in bracket form: `$["first name"]`.
///
/// # Errors
/// Returns [`DiffError::Serialization`] if either string is not valid JSON.
pub fn json_diff(old_json: &str, new_json: &str) -> Result<Vec<JsonDiffOp>, DiffError> {
    let old: serde_json::Value = serde_json::from_str(old_json)?;
    let new: serde_json::Value = serde_json::from_str(new_json)?;
    Ok(json_diff_values(&old, &new))
}

/// Same as [`json_diff`] for values you have already parsed.
pub fn json_diff_values(old: &serde_json::Value, new: &serde_json::Value) -> Vec<JsonDiffOp> {
    let mut ops = Vec::new();
    diff_values("$", old, new, &mut ops);
    if ops.is_empty() {
        ops.push(JsonDiffOp::Equal);
    }
    ops
}

/// How one array element moved between the old and new array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArrayOp {
    Same { old_index: usize, new_index: usize },
    Removed { old_index: usize },
    Added { new_index: usize },
    Changed { old_index: usize, new_index: usize },
}

/// Align two arrays with a Myers diff over the elements' canonical JSON text.
/// Inside a replaced block, elements are paired up in order and reported as
/// changed; leftovers are removals or additions.
fn array_ops(old: &[serde_json::Value], new: &[serde_json::Value]) -> Vec<ArrayOp> {
    let key = |v: &serde_json::Value| serde_json::to_string(v).unwrap_or_default();
    let ok: Vec<String> = old.iter().map(key).collect();
    let nk: Vec<String> = new.iter().map(key).collect();
    let mut out = Vec::new();
    for op in similar::capture_diff_slices(similar::Algorithm::Myers, &ok, &nk) {
        match op {
            similar::DiffOp::Equal { old_index, new_index, len } => {
                out.extend((0..len).map(|i| ArrayOp::Same { old_index: old_index + i, new_index: new_index + i }));
            }
            similar::DiffOp::Delete { old_index, old_len, .. } => {
                out.extend((old_index..old_index + old_len).map(|old_index| ArrayOp::Removed { old_index }));
            }
            similar::DiffOp::Insert { new_index, new_len, .. } => {
                out.extend((new_index..new_index + new_len).map(|new_index| ArrayOp::Added { new_index }));
            }
            similar::DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                let paired = old_len.min(new_len);
                out.extend((0..paired).map(|i| ArrayOp::Changed { old_index: old_index + i, new_index: new_index + i }));
                out.extend((old_index + paired..old_index + old_len).map(|old_index| ArrayOp::Removed { old_index }));
                out.extend((new_index + paired..new_index + new_len).map(|new_index| ArrayOp::Added { new_index }));
            }
        }
    }
    out
}

/// An [RFC 6902](https://www.rfc-editor.org/rfc/rfc6902) JSON Patch that turns
/// `old` into `new`, as a JSON array of `add` / `remove` / `replace`
/// operations. Any JSON Patch library (the `json-patch` crate, `fast-json-patch`
/// in JavaScript, `jsonpatch` in Python) can apply it.
///
/// Arrays are aligned the same way as in [`json_diff`], so inserting at the
/// front of a list is one `add`, not a rewrite of the whole list. Operations
/// are ordered so that applying them one after another is correct.
///
/// ```
/// use serde_json::json;
/// let patch = llm_diff::json_patch(&json!({"tags": ["b"]}), &json!({"tags": ["a", "b"]}));
/// assert_eq!(patch, json!([{"op": "add", "path": "/tags/0", "value": "a"}]));
/// ```
pub fn json_patch(old: &serde_json::Value, new: &serde_json::Value) -> serde_json::Value {
    let mut ops = Vec::new();
    patch_values("", old, new, &mut ops);
    serde_json::Value::Array(ops)
}

fn pointer_escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn patch_values(ptr: &str, old: &serde_json::Value, new: &serde_json::Value, ops: &mut Vec<serde_json::Value>) {
    use serde_json::{json, Value};
    match (old, new) {
        (Value::Object(o), Value::Object(n)) => {
            for (k, ov) in o {
                let child = format!("{ptr}/{}", pointer_escape(k));
                match n.get(k) {
                    Some(nv) => patch_values(&child, ov, nv, ops),
                    None => ops.push(json!({"op": "remove", "path": child})),
                }
            }
            for (k, nv) in n {
                if !o.contains_key(k) {
                    ops.push(json!({"op": "add", "path": format!("{ptr}/{}", pointer_escape(k)), "value": nv}));
                }
            }
        }
        (Value::Array(o), Value::Array(n)) => {
            // Walk in order, tracking where each element sits in the array as
            // it is being patched (removals shift later elements left).
            let mut at = 0usize;
            for op in array_ops(o, n) {
                match op {
                    ArrayOp::Same { .. } => at += 1,
                    ArrayOp::Removed { .. } => ops.push(json!({"op": "remove", "path": format!("{ptr}/{at}")})),
                    ArrayOp::Added { new_index } => {
                        ops.push(json!({"op": "add", "path": format!("{ptr}/{at}"), "value": n[new_index]}));
                        at += 1;
                    }
                    ArrayOp::Changed { old_index, new_index } => {
                        patch_values(&format!("{ptr}/{at}"), &o[old_index], &n[new_index], ops);
                        at += 1;
                    }
                }
            }
        }
        (o, n) if o == n => {}
        (_, n) => ops.push(json!({"op": "replace", "path": ptr, "value": n})),
    }
}

fn key_path(parent: &str, key: &str) -> String {
    let plain = !key.is_empty()
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !key.starts_with(|c: char| c.is_ascii_digit());
    if plain {
        format!("{parent}.{key}")
    } else {
        // serde_json escapes quotes and backslashes for us.
        format!("{parent}[{}]", serde_json::Value::String(key.to_string()))
    }
}

fn diff_values(path: &str, old: &serde_json::Value, new: &serde_json::Value, ops: &mut Vec<JsonDiffOp>) {
    use serde_json::Value;
    match (old, new) {
        (Value::Object(o), Value::Object(n)) => {
            for (k, ov) in o {
                let child_path = key_path(path, k);
                match n.get(k) {
                    Some(nv) => diff_values(&child_path, ov, nv, ops),
                    None => ops.push(JsonDiffOp::KeyRemoved { path: child_path, value: ov.clone() }),
                }
            }
            for (k, nv) in n {
                if !o.contains_key(k) {
                    ops.push(JsonDiffOp::KeyAdded { path: key_path(path, k), value: nv.clone() });
                }
            }
        }
        (Value::Array(o), Value::Array(n)) => {
            for op in array_ops(o, n) {
                match op {
                    ArrayOp::Same { .. } => {}
                    ArrayOp::Removed { old_index } => ops.push(JsonDiffOp::KeyRemoved {
                        path: format!("{path}[{old_index}]"),
                        value: o[old_index].clone(),
                    }),
                    ArrayOp::Added { new_index } => ops.push(JsonDiffOp::KeyAdded {
                        path: format!("{path}[{new_index}]"),
                        value: n[new_index].clone(),
                    }),
                    ArrayOp::Changed { old_index, new_index } => {
                        diff_values(&format!("{path}[{new_index}]"), &o[old_index], &n[new_index], ops)
                    }
                }
            }
        }
        (o, n) if o == n => {}
        (o, n) => ops.push(JsonDiffOp::ValueChanged {
            path: path.to_string(),
            old: o.clone(),
            new: n.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_diff_identical_strings_similarity_one() {
        let d = TextDiff::compute("hello", "hello");
        assert!(d.is_identical());
        assert_eq!(d.similarity, 1.0);
    }

    #[test]
    fn test_text_diff_completely_different_similarity_less_than_one() {
        let d = TextDiff::compute("aaa bbb ccc", "xxx yyy zzz");
        assert!(d.similarity < 1.0);
    }

    #[test]
    fn test_text_diff_insertions_counted() {
        let d = TextDiff::compute("line1", "line1\nline2");
        assert!(d.insertions() > 0);
    }

    #[test]
    fn test_text_diff_deletions_counted() {
        let d = TextDiff::compute("line1\nline2", "line1");
        assert!(d.deletions() > 0);
    }

    #[test]
    fn test_text_diff_similarity_in_range() {
        let d = TextDiff::compute("the quick brown fox", "the slow blue dog");
        assert!(d.similarity >= 0.0 && d.similarity <= 1.0);
    }

    #[test]
    fn test_diff_op_kind_labels() {
        assert_eq!(DiffOp::Equal("x".into()).kind(), "=");
        assert_eq!(DiffOp::Insert("x".into()).kind(), "+");
        assert_eq!(DiffOp::Delete("x".into()).kind(), "-");
    }

    #[test]
    fn test_diff_op_text_returns_content() {
        assert_eq!(DiffOp::Insert("hello".into()).text(), "hello");
    }

    #[test]
    fn test_json_diff_equal_returns_equal_op() {
        let ops = json_diff(r#"{"a":1}"#, r#"{"a":1}"#).unwrap();
        assert!(matches!(ops[0], JsonDiffOp::Equal));
    }

    #[test]
    fn test_json_diff_value_changed_detected() {
        let ops = json_diff(r#"{"a":1}"#, r#"{"a":2}"#).unwrap();
        assert!(ops.iter().any(|op| matches!(op, JsonDiffOp::ValueChanged { .. })));
    }

    #[test]
    fn test_json_diff_key_added_detected() {
        let ops = json_diff(r#"{"a":1}"#, r#"{"a":1,"b":2}"#).unwrap();
        assert!(ops.iter().any(|op| matches!(op, JsonDiffOp::KeyAdded { .. })));
    }

    #[test]
    fn test_json_diff_key_removed_detected() {
        let ops = json_diff(r#"{"a":1,"b":2}"#, r#"{"a":1}"#).unwrap();
        assert!(ops.iter().any(|op| matches!(op, JsonDiffOp::KeyRemoved { .. })));
    }

    #[test]
    fn test_json_diff_invalid_json_returns_serialization_error() {
        let err = json_diff("not json", "{}").unwrap_err();
        assert!(matches!(err, DiffError::Serialization(_)));
    }

    #[test]
    fn test_text_diff_empty_strings_identical() {
        let d = TextDiff::compute("", "");
        assert!(d.is_identical());
    }

    // ── 0.2.0 regression tests ───────────────────────────────────────────────

    #[test]
    fn test_reordered_words_are_not_identical() {
        // 0.1.x decided "identical" from the word-set similarity, so a reordered
        // answer was reported as identical.
        let d = TextDiff::compute("yes no", "no yes");
        assert!(!d.is_identical());
        assert_eq!(d.similarity, 1.0);
        assert!(d.ratio < 1.0);
    }

    #[test]
    fn test_trailing_newline_change_is_detected() {
        let d = TextDiff::compute("a\nb\n", "a\nb");
        assert!(!d.is_identical());
        assert_eq!(d.insertions(), 1);
        assert_eq!(d.deletions(), 1);
    }

    #[test]
    fn test_identical_text_gives_line_ops() {
        let d = TextDiff::compute("x\ny", "x\ny");
        assert!(d.is_identical());
        assert_eq!(d.ops, vec![DiffOp::Equal("x".into()), DiffOp::Equal("y".into())]);
        assert_eq!(d.ratio, 1.0);
    }

    #[test]
    fn test_line_ops_match_expected_sequence() {
        let d = TextDiff::compute("a\nb\nc", "a\nx\nc");
        assert_eq!(
            d.ops,
            vec![
                DiffOp::Equal("a".into()),
                DiffOp::Delete("b".into()),
                DiffOp::Insert("x".into()),
                DiffOp::Equal("c".into()),
            ]
        );
    }

    #[test]
    fn test_crlf_line_endings_are_stripped() {
        let d = TextDiff::compute("a\r\nb\r\n", "a\r\nc\r\n");
        assert!(d.ops.contains(&DiffOp::Equal("a".into())));
        assert!(d.ops.contains(&DiffOp::Insert("c".into())));
    }

    #[test]
    fn test_word_diff_on_single_paragraph() {
        let old = "The capital of France is Paris.";
        let new = "The capital of France is Lyon.";
        let d = TextDiff::compute_words(old, new);
        assert!(d.ops.contains(&DiffOp::Delete("Paris.".into())));
        assert!(d.ops.contains(&DiffOp::Insert("Lyon.".into())));
        let rebuilt: String = d
            .ops
            .iter()
            .filter(|o| !matches!(o, DiffOp::Delete(_)))
            .map(DiffOp::text)
            .collect();
        assert_eq!(rebuilt, new);
        // A line diff sees the whole paragraph replaced.
        assert_eq!(TextDiff::compute(old, new).ratio, 0.0);
        assert!(d.ratio > 0.8);
    }

    #[test]
    fn test_large_inputs_diff_quickly() {
        // 0.1.x built an (n+1) x (m+1) table of usize: 20,000 lines each side
        // would need 3.2 GB. Myers needs memory proportional to the input.
        let old: String = (0..20_000).map(|i| format!("line {i}\n")).collect();
        let new = old.replace("line 10000\n", "changed\n");
        let d = TextDiff::compute(&old, &new);
        assert_eq!(d.insertions(), 1);
        assert_eq!(d.deletions(), 1);
    }

    #[test]
    fn test_unified_diff_format() {
        assert_eq!(unified_diff("same", "same", 3, "a", "b"), "");
        let u = unified_diff("one\ntwo\n", "one\nthree\n", 3, "old.txt", "new.txt");
        assert!(u.starts_with("--- old.txt\n+++ new.txt\n@@"));
        assert!(u.contains("-two\n") && u.contains("+three\n"));
    }

    #[test]
    fn test_json_diff_recurses_into_arrays() {
        // 0.1.x reported the whole array as one changed value.
        let ops = json_diff(r#"{"items":[{"p":1},{"p":2}]}"#, r#"{"items":[{"p":1},{"p":3},{"p":4}]}"#).unwrap();
        assert_eq!(
            ops,
            vec![
                JsonDiffOp::ValueChanged { path: "$.items[1].p".into(), old: 2.into(), new: 3.into() },
                JsonDiffOp::KeyAdded { path: "$.items[2]".into(), value: serde_json::json!({"p": 4}) },
            ]
        );
        let ops = json_diff("[1,2,3]", "[1]").unwrap();
        assert_eq!(ops.len(), 2);
        assert!(matches!(&ops[0], JsonDiffOp::KeyRemoved { path, .. } if path == "$[1]"));
    }

    #[test]
    fn test_json_diff_quotes_awkward_keys() {
        let ops = json_diff(r#"{"a.b":1,"first name":"x"}"#, r#"{"a.b":2}"#).unwrap();
        assert!(ops.contains(&JsonDiffOp::ValueChanged { path: "$[\"a.b\"]".into(), old: 1.into(), new: 2.into() }));
        assert!(matches!(&ops[1], JsonDiffOp::KeyRemoved { path, .. } if path == "$[\"first name\"]"));
    }

    #[test]
    fn test_json_diff_type_change() {
        let ops = json_diff(r#"{"a":[1]}"#, r#"{"a":{"0":1}}"#).unwrap();
        assert!(matches!(&ops[0], JsonDiffOp::ValueChanged { path, .. } if path == "$.a"));
    }

    // ── array alignment, JSON Patch, char and unicode diffs ─────────────────

    #[test]
    fn test_json_array_insert_at_front_is_one_add() {
        // 0.2.0-dev compared arrays by index, so this showed 3 changes and an add.
        let ops = json_diff(r#"[{"id":1},{"id":2},{"id":3}]"#, r#"[{"id":0},{"id":1},{"id":2},{"id":3}]"#).unwrap();
        assert_eq!(ops, vec![JsonDiffOp::KeyAdded { path: "$[0]".into(), value: serde_json::json!({"id": 0}) }]);
    }

    #[test]
    fn test_json_array_duplicates_and_removal_in_middle() {
        let ops = json_diff("[1,1,1]", "[1,1]").unwrap();
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], JsonDiffOp::KeyRemoved { .. }));
        let ops = json_diff(r#"["a","b","c"]"#, r#"["a","c"]"#).unwrap();
        assert_eq!(ops, vec![JsonDiffOp::KeyRemoved { path: "$[1]".into(), value: "b".into() }]);
    }

    #[test]
    fn test_json_patch_escapes_pointer_tokens() {
        use serde_json::json;
        let p = json_patch(&json!({"a/b": 1, "m~n": 1}), &json!({"a/b": 2}));
        assert_eq!(
            p,
            json!([{"op": "replace", "path": "/a~1b", "value": 2}, {"op": "remove", "path": "/m~0n"}])
        );
        assert_eq!(json_patch(&json!([1, 2]), &json!([1, 2])), json!([]));
        assert_eq!(json_patch(&json!(1), &json!("x")), json!([{"op": "replace", "path": "", "value": "x"}]));
    }

    #[test]
    fn test_char_diff() {
        let d = TextDiff::compute_chars("2.1M", "2.2M");
        assert_eq!(d.insertions(), 1);
        assert_eq!(d.deletions(), 1);
        assert!(d.ops.contains(&DiffOp::Delete("1".into())));
    }

    #[cfg(feature = "unicode")]
    #[test]
    fn test_unicode_words_split_text_without_spaces() {
        let (old, new) = ("我喜欢猫。", "我喜欢狗。");
        // whitespace splitting sees one "word" and replaces all of it
        let ws = TextDiff::compute_words(old, new);
        assert_eq!(ws.ratio, 0.0);
        let uw = TextDiff::compute_unicode_words(old, new);
        assert!(uw.ratio > 0.5, "{}", uw.ratio);
        assert!(uw.ops.contains(&DiffOp::Equal("我".into())));
        assert!(uw.ops.contains(&DiffOp::Insert("狗".into())));
        // punctuation is its own token
        let d = TextDiff::compute_unicode_words("Paris, France.", "Lyon, France.");
        assert!(d.ops.contains(&DiffOp::Delete("Paris".into())));
        assert!(d.ops.contains(&DiffOp::Equal(",".into())));
    }

    #[cfg(feature = "patch")]
    #[test]
    fn test_our_patches_apply_with_diffy() {
        // Cross-check: a patch produced by similar is applied by diffy.
        let cases = [
            ("a\nb\nc\n", "a\nB\nc\n"),
            ("one\ntwo\n", "one\ntwo"),          // newline removed at end
            ("one\ntwo", "one\ntwo\nthree\n"),   // no newline at end originally
            ("", "new file\n"),
            ("gone\n", ""),
        ];
        for (old, new) in cases {
            let p = unified_diff(old, new, 3, "a", "b");
            assert_eq!(apply_patch(old, &p).unwrap(), new, "patch:\n{p}");
        }
        assert!(apply_patch("x\n", "not a patch").is_err());
        let p = unified_diff("a\nb\n", "a\nc\n", 1, "a", "b");
        assert!(apply_patch("totally\ndifferent\n", &p).is_err());
    }

    #[cfg(feature = "patch")]
    #[test]
    fn test_merge3() {
        let base = "intro\nbody\noutro\n";
        let ours = "INTRO\nbody\noutro\n";
        let theirs = "intro\nbody\nOUTRO\n";
        assert_eq!(merge3(base, ours, theirs).unwrap(), "INTRO\nbody\nOUTRO\n");
        match merge3(base, "x\nbody\noutro\n", "y\nbody\noutro\n") {
            Err(DiffError::MergeConflict(text)) => {
                assert!(text.contains("<<<<<<<") && text.contains("x") && text.contains("y"));
            }
            other => panic!("expected a conflict, got {other:?}"),
        }
        // like git, edits on directly adjacent lines conflict too
        assert!(merge3("a
b
", "A
b
", "a
B
").is_err());
        assert_eq!(merge3("a
x
b
", "A
x
b
", "a
x
B
").unwrap(), "A
x
B
");
    }
}
