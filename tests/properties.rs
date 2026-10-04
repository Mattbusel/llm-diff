//! Property tests and cross-checks against other crates.
#![allow(clippy::unwrap_used)]

use llm_diff::{json_diff, json_patch, unified_diff, DiffOp, JsonDiffOp, TextDiff};
use proptest::prelude::*;
use serde_json::Value;

/// Rebuild both sides from the ops of a word or char diff (exact, no line endings stripped).
fn rebuild(d: &TextDiff) -> (String, String) {
    let mut old = String::new();
    let mut new = String::new();
    for op in &d.ops {
        match op {
            DiffOp::Equal(t) => {
                old.push_str(t);
                new.push_str(t);
            }
            DiffOp::Delete(t) => old.push_str(t),
            DiffOp::Insert(t) => new.push_str(t),
        }
    }
    (old, new)
}

fn text() -> impl Strategy<Value = String> {
    // small alphabet so diffs have both matches and changes; includes
    // multi-byte characters, CR and LF
    prop::collection::vec(prop::sample::select(vec!["a", "b", " ", "\n", "\r\n", "é", "猫", "x y", ".", "\t"]), 0..40)
        .prop_map(|v| v.concat())
}

fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        (0i64..5).prop_map(Value::from),
        prop::sample::select(vec!["a", "b", "a/b", "m~n", ""]).prop_map(Value::from),
    ];
    leaf.prop_recursive(4, 40, 5, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..5).prop_map(Value::Array),
            prop::collection::btree_map(prop::sample::select(vec!["k", "v", "a.b", "x y", "~", "/"]).prop_map(String::from), inner, 0..4)
                .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn word_and_char_ops_rebuild_both_texts(a in text(), b in text()) {
        for d in [TextDiff::compute_words(&a, &b), TextDiff::compute_chars(&a, &b)] {
            let (old, new) = rebuild(&d);
            prop_assert_eq!(&old, &a);
            prop_assert_eq!(&new, &b);
            prop_assert!((0.0..=1.0).contains(&d.ratio));
            prop_assert_eq!(d.is_identical(), a == b);
        }
    }

    #[test]
    fn line_ops_rebuild_lines(a in text(), b in text()) {
        let d = TextDiff::compute(&a, &b);
        let old: Vec<String> = d.ops.iter().filter(|o| !matches!(o, DiffOp::Insert(_))).map(|o| o.text().to_string()).collect();
        let new: Vec<String> = d.ops.iter().filter(|o| !matches!(o, DiffOp::Delete(_))).map(|o| o.text().to_string()).collect();
        let lines = |s: &str| s.lines().map(str::to_string).collect::<Vec<_>>();
        prop_assert_eq!(old, lines(&a));
        prop_assert_eq!(new, lines(&b));
        prop_assert_eq!(d.is_identical(), a == b);
    }

    /// Our JSON Patch, applied by the json-patch crate, turns old into new.
    #[test]
    fn json_patch_applies_with_json_patch_crate(a in json_value(), b in json_value()) {
        let ours = json_patch(&a, &b);
        let patch: json_patch::Patch = serde_json::from_value(ours.clone()).unwrap();
        let mut doc = a.clone();
        json_patch::patch(&mut doc, &patch).unwrap();
        prop_assert_eq!(&doc, &b, "patch was {}", ours);
    }

    /// No diff of a value with itself; some diff otherwise.
    #[test]
    fn json_diff_detects_exactly_the_changes(a in json_value(), b in json_value()) {
        let ops = json_diff(&a.to_string(), &b.to_string()).unwrap();
        prop_assert_eq!(ops == vec![JsonDiffOp::Equal], a == b);
    }

    #[test]
    fn never_panics_on_any_strings(a in ".{0,60}", b in ".{0,60}") {
        let _ = TextDiff::compute(&a, &b);
        let _ = unified_diff(&a, &b, 2, "a", "b");
        let _ = json_diff(&a, &b);
    }
}

#[cfg(feature = "patch")]
proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    /// Unified diffs from similar apply cleanly with diffy.
    #[test]
    fn unified_diff_round_trips_through_diffy(a in text(), b in text()) {
        // diffy works on lines; normalise CRLF like a text editor would.
        let a = a.replace('\r', "");
        let b = b.replace('\r', "");
        let p = unified_diff(&a, &b, 3, "a", "b");
        prop_assert_eq!(llm_diff::apply_patch(&a, &p).unwrap(), b);
    }
}

#[test]
fn json_patch_matches_json_patch_crate_on_list_insert() {
    // json-patch's own diff compares arrays by index; ours aligns them.
    let a: Value = serde_json::json!({"items": [1, 2, 3, 4, 5]});
    let b: Value = serde_json::json!({"items": [0, 1, 2, 3, 4, 5]});
    let ours = json_patch(&a, &b);
    let theirs = serde_json::to_value(json_patch::diff(&a, &b)).unwrap();
    assert_eq!(ours.as_array().unwrap().len(), 1);
    assert!(theirs.as_array().unwrap().len() > 1, "{theirs}");
}
