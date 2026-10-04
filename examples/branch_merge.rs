//! Two people improve the same model answer on different branches; merge
//! their edits like git does, and ship the result as a patch.
//!
//! Run with `cargo run --example branch_merge --features patch`.

use llm_diff::{apply_patch, DiffError, OutputVersion, VersionAnnotation, VersionStore};

fn main() -> Result<(), DiffError> {
    let mut store = VersionStore::new(10_000);
    let base = "Title: Paris guide\nGetting around: metro\nWeather: mild\nFood: bistros\n";
    let root = store.store(OutputVersion::new(base, "gpt-4o", VersionAnnotation::default(), None))?;

    let note = |n: &str| VersionAnnotation { note: Some(n.into()), ..Default::default() };
    let alice = store.store(OutputVersion::new(
        "Title: Paris guide\nGetting around: metro and RER\nWeather: mild\nFood: bistros\n",
        "gpt-4o",
        note("alice: transport"),
        Some(root.clone()),
    ))?;
    let bob = store.store(OutputVersion::new(
        "Title: Paris guide\nGetting around: metro\nWeather: mild\nFood: bistros and bakeries\n",
        "claude-sonnet-4-5",
        note("bob: food"),
        Some(root.clone()),
    ))?;
    store.set_branch("alice", alice.as_str())?;
    store.set_branch("bob", bob.as_str())?;

    let merged = store.merge(&root, &alice, &bob)?;
    println!("merged:\n{merged}");

    // The change from the original, as a patch anyone can apply with `patch -p0` or git.
    let patch = llm_diff::unified_diff(base, &merged, 1, "guide.txt", "guide.txt");
    print!("{patch}");
    assert_eq!(apply_patch(base, &patch)?, merged);

    // Overlapping edits produce a conflict with git-style markers.
    let carol = store.store(OutputVersion::new(
        "Title: Paris guide\nGetting around: bikes\nWeather: mild\nFood: bistros\n",
        "gpt-4o",
        note("carol"),
        Some(root.clone()),
    ))?;
    match store.merge(&root, &alice, &carol) {
        Err(DiffError::MergeConflict(text)) => println!("conflict, resolve by hand:\n{text}"),
        other => println!("unexpected: {other:?}"),
    }
    Ok(())
}
