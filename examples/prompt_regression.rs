//! Re-run a prompt after changing it and see how the answers moved: a word
//! diff for single-paragraph answers, a git-style patch for longer ones, and
//! scores that flag answers that changed a lot.
//!
//! Run with `cargo run --example prompt_regression`.

use llm_diff::{unified_diff, DiffOp, TextDiff};

fn main() {
    // (question, answer with prompt v1, answer with prompt v2)
    let runs = [
        ("capital", "The capital of France is Paris.", "The capital of France is Paris."),
        ("river", "Paris sits on the Seine, which flows into the English Channel.", "Paris sits on the river Seine, which flows to the English Channel."),
        ("numbers", "Paris has about 2.1 million people.", "Paris has about 2.2 million people."),
        ("rewrite", "Use the metro.\nBuy a carnet.\n", "Buy a Navigo pass.\nUse the metro or RER.\n"),
    ];

    for (name, v1, v2) in runs {
        let words = TextDiff::compute_words(v1, v2);
        let flag = if words.is_identical() { "same" } else if words.ratio < 0.5 { "REVIEW" } else { "changed" };
        println!("[{flag:>7}] {name}: ratio {:.2}, word overlap {:.2}", words.ratio, words.similarity);
        for op in words.ops.iter().filter(|o| !matches!(o, DiffOp::Equal(_)) && !o.text().trim().is_empty()) {
            println!("           {} {:?}", op.kind(), op.text());
        }
        if v1.lines().count() > 1 && v1 != v2 {
            print!("{}", unified_diff(v1, v2, 1, "prompt-v1", "prompt-v2"));
        }
    }
    // "2.1" -> "2.2" is one token; a character diff shows exactly which digit moved.
    let d = TextDiff::compute_chars("2.1 million", "2.2 million");
    println!("char diff: -{} +{}", d.deletions(), d.insertions());
}
