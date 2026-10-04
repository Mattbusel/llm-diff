//! Line and word diffs against the other Rust diff crates, on the same inputs.
//!
//! - llm-diff 0.2 `TextDiff::compute` / `compute_words` (similar, Myers, plus
//!   owned `Vec<DiffOp>` of `String`s and a Jaccard score)
//! - llm-diff 0.1.1 `TextDiff::compute` (hand-written O(n*m) LCS)
//! - similar 3 directly (`TextDiff::from_lines(..).ops()`)
//! - imara-diff 0.2 (Histogram, with interning)
//! - diffy 0.5 `create_patch`
//! - dissimilar 1 (Google diff-match-patch port, character based, semantic cleanup)
//!
//! Inputs are generated deterministically: "answer" is 300 lines with every
//! 20th line edited, "doc" is 5,000 lines with every 100th edited, "paragraph"
//! is one 2,000-word line with every 25th word changed.
//!
//! Run: `cargo bench --bench vs_alternatives`
#![allow(clippy::unwrap_used)]

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

const WORDS: &[&str] = &[
    "the", "model", "returns", "a", "summary", "of", "Paris", "with", "population", "data", "and", "sources",
    "for", "each", "claim", "in", "its", "answer", "which", "may", "change", "when", "prompt", "temperature",
];

fn word(seed: &mut u64) -> &'static str {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    WORDS[(*seed >> 33) as usize % WORDS.len()]
}

fn lines(n: usize, every: usize) -> (String, String) {
    let mut seed = 42;
    let mut old = String::new();
    let mut new = String::new();
    for i in 0..n {
        let line: Vec<&str> = (0..10).map(|_| word(&mut seed)).collect();
        let line = line.join(" ");
        old.push_str(&line);
        old.push('\n');
        if i % every == every - 1 {
            new.push_str(&line.replacen("the", "THE", 1));
            new.push_str(" edited");
        } else {
            new.push_str(&line);
        }
        new.push('\n');
    }
    (old, new)
}

fn paragraph(n: usize, every: usize) -> (String, String) {
    let mut seed = 7;
    let words: Vec<&str> = (0..n).map(|_| word(&mut seed)).collect();
    let old = words.join(" ");
    let new: Vec<&str> = words.iter().enumerate().map(|(i, w)| if i % every == every - 1 { "CHANGED" } else { *w }).collect();
    (old, new.join(" "))
}

fn line_diffs(c: &mut Criterion) {
    let mut g = c.benchmark_group("line diff");
    for (name, (old, new)) in [("answer 300 lines", lines(300, 20)), ("doc 5000 lines", lines(5_000, 100))] {
        g.bench_with_input(BenchmarkId::new("llm-diff 0.2", name), &(&old, &new), |b, (o, n)| {
            b.iter(|| black_box(llm_diff::TextDiff::compute(o, n)))
        });
        g.bench_with_input(BenchmarkId::new("llm-diff 0.1.1", name), &(&old, &new), |b, (o, n)| {
            b.iter(|| black_box(llm_diff_011::TextDiff::compute(o, n)))
        });
        g.bench_with_input(BenchmarkId::new("similar", name), &(&old, &new), |b, (o, n)| {
            b.iter(|| black_box(similar::TextDiff::from_lines(o.as_str(), n.as_str()).ops().len()))
        });
        g.bench_with_input(BenchmarkId::new("imara-diff", name), &(&old, &new), |b, (o, n)| {
            b.iter(|| {
                let input = imara_diff::InternedInput::new(o.as_str(), n.as_str());
                let mut d = imara_diff::Diff::compute(imara_diff::Algorithm::Histogram, &input);
                d.postprocess_lines(&input);
                black_box(d.count_additions())
            })
        });
        g.bench_with_input(BenchmarkId::new("diffy", name), &(&old, &new), |b, (o, n)| {
            b.iter(|| black_box(diffy::create_patch(o, n).hunks().len()))
        });
    }
    g.finish();
}

fn word_diffs(c: &mut Criterion) {
    let mut g = c.benchmark_group("word diff");
    let (old, new) = paragraph(2_000, 25);
    g.bench_function("llm-diff 0.2 compute_words", |b| b.iter(|| black_box(llm_diff::TextDiff::compute_words(&old, &new))));
    g.bench_function("similar from_words", |b| {
        b.iter(|| black_box(similar::TextDiff::from_words(old.as_str(), new.as_str()).ops().len()))
    });
    g.bench_function("dissimilar (chars, semantic cleanup)", |b| b.iter(|| black_box(dissimilar::diff(&old, &new).len())));
    g.finish();
}

criterion_group!(benches, line_diffs, word_diffs);
criterion_main!(benches);
