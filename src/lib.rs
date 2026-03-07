// SPDX-License-Identifier: MIT
//! # llm-diff
//!
//! Output diffing and versioning primitives for LLM outputs.
//!
//! Provides:
//! - Line-level and structural JSON diffing ([`diff`] module).
//! - A content-addressable version store with branch and lineage support ([`store`] module).
//! - An append-only compliance audit log ([`audit`] module).

pub mod audit;
pub mod diff;
pub mod error;
pub mod store;

pub use audit::{AuditEvent, AuditLog};
pub use diff::{json_diff, DiffOp, TextDiff};
pub use error::DiffError;
pub use store::{OutputVersion, VersionAnnotation, VersionStore};
