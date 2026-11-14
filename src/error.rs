// SPDX-License-Identifier: MIT
use thiserror::Error;

/// All errors that can occur in the llm-diff library.
///
/// The enum is `#[non_exhaustive]`: match it with a wildcard arm, because new
/// variants can be added in minor releases. The store-related variants exist
/// only with the `store` feature (on by default).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DiffError {
    /// A requested version ID was not found in the store.
    #[cfg(feature = "store")]
    #[cfg_attr(docsrs, doc(cfg(feature = "store")))]
    #[error("version '{0}' not found in store")]
    VersionNotFound(String),
    /// An output is bigger than the store's size limit
    /// ([`VersionStore::new`](crate::VersionStore::new)).
    #[cfg(feature = "store")]
    #[cfg_attr(docsrs, doc(cfg(feature = "store")))]
    #[error("output too large: {size} bytes exceeds the limit of {limit} bytes")]
    OutputTooLarge {
        /// Size of the output in bytes.
        size: usize,
        /// The store's limit in bytes.
        limit: usize,
    },
    /// A version with this ID but different content is already stored.
    /// Version IDs must be unique; create a new [`OutputVersion`](crate::OutputVersion)
    /// instead of reusing one.
    #[cfg(feature = "store")]
    #[cfg_attr(docsrs, doc(cfg(feature = "store")))]
    #[error("version id '{0}' is already used by different content")]
    DuplicateVersionId(String),
    /// A saved store (see [`VersionStore::from_json`](crate::VersionStore::from_json))
    /// is internally inconsistent, for example a branch points at a version
    /// that is not in it.
    #[cfg(feature = "store")]
    #[cfg_attr(docsrs, doc(cfg(feature = "store")))]
    #[error("invalid saved store: {0}")]
    InvalidStore(String),
    /// A branch name was not found in the store.
    #[cfg(feature = "store")]
    #[cfg_attr(docsrs, doc(cfg(feature = "store")))]
    #[error("branch '{0}' not found")]
    BranchNotFound(String),
    /// JSON serialization or deserialization failed.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    /// A diff or patch was structurally invalid.
    #[error("invalid diff: {0}")]
    InvalidDiff(String),
    /// A three-way merge found overlapping edits. Holds the merged text with
    /// git-style conflict markers.
    #[error("merge conflict")]
    MergeConflict(String),
}
