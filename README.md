# llm-diff

Output diffing and versioning primitives for LLM outputs — semantic diff, version store, and lineage tracking.

Compare two LLM responses, track how outputs evolve across prompt iterations, and build audit trails for prompt engineering workflows.

## What's inside

- **SemanticDiff** — word-level and structural diff between two LLM outputs
- **VersionStore** — append-only output history keyed by prompt hash
- **Lineage** — parent/child relationships between prompt versions
- **DiffReport** — structured diff result: added, removed, changed sections with positions

## Use cases

- Prompt regression testing — detect when a prompt change causes output drift
- A/B output comparison — structured diff between two model responses
- Audit trails — record all outputs for a given prompt over time
- Change detection — trigger alerts when output structure changes significantly

## Quick start

```rust
use llm_diff::{SemanticDiff, VersionStore};

let diff = SemanticDiff::compute(
    "The capital of France is Paris.",
    "The capital of France is Lyon.",
);
println!("Changed tokens: {:?}", diff.changed);

let store = VersionStore::new();
store.record("prompt-hash-abc", "response v1")?;
store.record("prompt-hash-abc", "response v2")?;
let history = store.history("prompt-hash-abc")?;
```

## Add to your project

```toml
[dependencies]
llm-diff = { git = "https://github.com/Mattbusel/llm-diff" }
```

Or one-liner:

```ash
cargo add --git https://github.com/Mattbusel/llm-diff
```

## Test coverage

```bash
cargo test
```

---

> Used inside [tokio-prompt-orchestrator](https://github.com/Mattbusel/tokio-prompt-orchestrator) -- a production Rust orchestration layer for LLM pipelines. See the full [primitive library collection](https://github.com/Mattbusel/rust-crates).