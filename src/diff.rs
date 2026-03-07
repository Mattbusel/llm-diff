// SPDX-License-Identifier: MIT
//! Diff primitives: line-level text diff and structural JSON diff.

use serde::{Deserialize, Serialize};
use crate::error::DiffError;

/// A single edit operation in a text diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct TextDiff {
    /// Sequence of edit operations transforming old into new.
    pub ops: Vec<DiffOp>,
    /// Jaccard similarity on word bags: 0.0 = no overlap, 1.0 = identical.
    pub similarity: f64,
}

impl TextDiff {
    /// Computes a line-level diff between `old` and `new`.
    pub fn compute(old: &str, new: &str) -> Self {
        if old == new {
            return Self { ops: vec![DiffOp::Equal(old.to_string())], similarity: 1.0 };
        }
        let old_lines: Vec<&str> = old.lines().collect();
        let new_lines: Vec<&str> = new.lines().collect();
        let ops = line_diff(&old_lines, &new_lines);
        let similarity = compute_similarity(old, new);
        Self { ops, similarity }
    }

    /// Returns the number of inserted lines.
    pub fn insertions(&self) -> usize {
        self.ops.iter().filter(|op| matches!(op, DiffOp::Insert(_))).count()
    }

    /// Returns the number of deleted lines.
    pub fn deletions(&self) -> usize {
        self.ops.iter().filter(|op| matches!(op, DiffOp::Delete(_))).count()
    }

    /// Returns `true` if the two texts were identical.
    pub fn is_identical(&self) -> bool {
        (self.similarity - 1.0).abs() < f64::EPSILON
    }
}

/// Computes a line-level diff using LCS backtracking.
fn line_diff(old: &[&str], new: &[&str]) -> Vec<DiffOp> {
    let m = old.len();
    let n = new.len();
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            dp[i][j] = if old[i - 1] == new[j - 1] {
                dp[i - 1][j - 1] + 1
            } else {
                dp[i - 1][j].max(dp[i][j - 1])
            };
        }
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (m, n);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old[i - 1] == new[j - 1] {
            ops.push(DiffOp::Equal(old[i - 1].to_string()));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(DiffOp::Insert(new[j - 1].to_string()));
            j -= 1;
        } else {
            ops.push(DiffOp::Delete(old[i - 1].to_string()));
            i -= 1;
        }
    }
    ops.reverse();
    ops
}

/// Computes Jaccard similarity on word bags.
fn compute_similarity(a: &str, b: &str) -> f64 {
    use std::collections::HashSet;
    let words_a: HashSet<&str> = a.split_whitespace().collect();
    let words_b: HashSet<&str> = b.split_whitespace().collect();
    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();
    if union == 0 { 1.0 } else { intersection as f64 / union as f64 }
}

/// An operation in a structural JSON diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JsonDiffOp {
    /// A scalar value changed at the given JSON path.
    ValueChanged { path: String, old: serde_json::Value, new: serde_json::Value },
    /// A key was added to an object.
    KeyAdded { path: String, value: serde_json::Value },
    /// A key was removed from an object.
    KeyRemoved { path: String, value: serde_json::Value },
    /// No differences found.
    Equal,
}

/// Computes a structural diff between two JSON strings.
///
/// # Errors
/// Returns [`DiffError::Serialization`] if either string is not valid JSON.
pub fn json_diff(old_json: &str, new_json: &str) -> Result<Vec<JsonDiffOp>, DiffError> {
    let old: serde_json::Value = serde_json::from_str(old_json)?;
    let new: serde_json::Value = serde_json::from_str(new_json)?;
    let mut ops = Vec::new();
    diff_values("$", &old, &new, &mut ops);
    if ops.is_empty() {
        ops.push(JsonDiffOp::Equal);
    }
    Ok(ops)
}

fn diff_values(path: &str, old: &serde_json::Value, new: &serde_json::Value, ops: &mut Vec<JsonDiffOp>) {
    match (old, new) {
        (serde_json::Value::Object(o), serde_json::Value::Object(n)) => {
            for (k, ov) in o {
                let child_path = format!("{path}.{k}");
                if let Some(nv) = n.get(k) {
                    diff_values(&child_path, ov, nv, ops);
                } else {
                    ops.push(JsonDiffOp::KeyRemoved { path: child_path, value: ov.clone() });
                }
            }
            for (k, nv) in n {
                if !o.contains_key(k) {
                    ops.push(JsonDiffOp::KeyAdded { path: format!("{path}.{k}"), value: nv.clone() });
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
}
