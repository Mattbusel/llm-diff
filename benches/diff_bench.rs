// SPDX-License-Identifier: MIT
use criterion::{criterion_group, criterion_main, Criterion};
use llm_diff::diff::TextDiff;
use llm_diff::store::{OutputVersion, VersionAnnotation, VersionStore};

fn bench_text_diff(c: &mut Criterion) {
    let old = "The quick brown fox jumps over the lazy dog. ".repeat(50);
    let new = "The fast brown fox leaps over the sleepy dog. ".repeat(50);
    c.bench_function("text_diff_2000_chars", |b| b.iter(|| TextDiff::compute(&old, &new)));
}

fn bench_version_store_insert(c: &mut Criterion) {
    c.bench_function("version_store_insert", |b| {
        b.iter_with_setup(
            || VersionStore::new(100_000),
            |mut store| {
                let v = OutputVersion::new(
                    "output content here",
                    "claude-sonnet-4-6",
                    VersionAnnotation::default(),
                    None,
                );
                let _ = store.store(v);
            },
        );
    });
}

fn bench_content_address(c: &mut Criterion) {
    let content = "a".repeat(10_000);
    c.bench_function("store_100k_chars", |b| {
        b.iter_with_setup(
            || VersionStore::new(1_000_000),
            |mut store| {
                let v = OutputVersion::new(&content, "m", VersionAnnotation::default(), None);
                let _ = store.store(v);
            },
        );
    });
}

criterion_group!(benches, bench_text_diff, bench_version_store_insert, bench_content_address);
criterion_main!(benches);
