// SPDX-License-Identifier: MIT
#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]

pub mod audit;
pub mod diff;
/// The error type.
pub mod error;
pub mod store;

pub use audit::{AuditEvent, AuditLog};
pub use diff::{json_diff, json_diff_values, json_patch, unified_diff, DiffOp, JsonDiffOp, TextDiff};
#[cfg(feature = "patch")]
pub use diff::{apply_patch, merge3};

/// The [similar](https://docs.rs/similar) crate this one is built on, for
/// diff features not wrapped here (inline highlighting, custom algorithms).
pub use similar;
pub use error::DiffError;
pub use store::{content_address, OutputVersion, VersionAnnotation, VersionStore};
