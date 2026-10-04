// SPDX-License-Identifier: MIT
use thiserror::Error;

/// All errors that can occur in the llm-diff library.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DiffError {
    /// A requested version ID was not found in the store.
    #[error("Version '{0}' not found in store")]
    VersionNotFound(String),
    /// A rollback target version has no accessible path.
    #[error("Cannot rollback to version '{0}': no path exists")]
    RollbackFailed(String),
    /// An output exceeded the configured token size limit.
    #[error("Output too large: {size} tokens exceeds limit {limit}")]
    OutputTooLarge {
        /// Estimated tokens in the output.
        size: usize,
        /// The store's limit.
        limit: usize,
    },
    /// JSON serialization or deserialization failed.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    /// A diff was structurally invalid.
    #[error("Invalid diff: {0}")]
    InvalidDiff(String),
    /// Two different versions mapped to the same content address.
    #[error("Content address collision for '{0}'")]
    ContentAddressCollision(String),
    /// A three-way merge found overlapping edits. Holds the merged text with
    /// git-style conflict markers.
    #[error("Merge conflict")]
    MergeConflict(String),
    /// A branch name was not found in the store.
    #[error("Branch '{0}' not found")]
    BranchNotFound(String),
}
