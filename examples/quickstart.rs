//! The README quickstart. Run with `cargo run --example quickstart`.

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
