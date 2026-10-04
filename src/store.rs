// SPDX-License-Identifier: MIT
//! Content-addressable version store for LLM outputs.

use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::diff::TextDiff;
use crate::error::DiffError;

/// SHA-256 of the content as 64 lowercase hex characters.
///
/// 0.1.x used a 64-bit FNV-1a hash, which is fast but not collision resistant:
/// two different outputs could share an address and the second would shadow
/// the first in [`VersionStore::get_by_address`].
pub fn content_address(content: &str) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    let digest = Sha256::digest(content.as_bytes());
    let mut out = String::with_capacity(64);
    for byte in digest.iter() {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Metadata describing why a new version was created.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct VersionAnnotation {
    /// Whether the prompt changed relative to the parent.
    pub prompt_changed: bool,
    /// Whether the model changed relative to the parent.
    pub model_changed: bool,
    /// Whether the temperature changed relative to the parent.
    pub temperature_changed: bool,
    /// Free-form human note.
    pub note: Option<String>,
}


/// A stored version of an LLM output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct OutputVersion {
    /// Unique UUID for this version.
    pub id: String,
    /// SHA-256 of the content (hex), used for deduplication.
    pub content_address: String,
    /// The raw text content.
    pub content: String,
    /// Model that produced this output.
    pub model: String,
    /// UTC timestamp of creation.
    pub created_at: DateTime<Utc>,
    /// Why this version was created.
    pub annotation: VersionAnnotation,
    /// Optional parent version ID for lineage tracking.
    pub parent_id: Option<String>,
}

impl OutputVersion {
    /// Creates a new version, computing the content address automatically.
    pub fn new(
        content: impl Into<String>,
        model: impl Into<String>,
        annotation: VersionAnnotation,
        parent_id: Option<String>,
    ) -> Self {
        let content = content.into();
        let addr = content_address(&content);
        Self {
            id: Uuid::new_v4().to_string(),
            content_address: addr,
            content,
            model: model.into(),
            created_at: Utc::now(),
            annotation,
            parent_id,
        }
    }
}

/// Content-addressable version store with branch and lineage support.
///
/// The whole store serializes with serde (for example to JSON with
/// [`to_json`](Self::to_json)), so it can be saved and loaded between runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct VersionStore {
    versions: HashMap<String, OutputVersion>,
    by_address: HashMap<String, String>,
    branches: HashMap<String, String>,
    max_output_tokens: usize,
}

impl VersionStore {
    /// Creates a new store with the given maximum output token limit per version.
    pub fn new(max_output_tokens: usize) -> Self {
        Self {
            versions: HashMap::new(),
            by_address: HashMap::new(),
            branches: HashMap::new(),
            max_output_tokens,
        }
    }

    /// Stores a version, returning its ID.
    ///
    /// The size check estimates tokens as one per four bytes of text.
    ///
    /// # Errors
    /// - [`DiffError::OutputTooLarge`] if the content exceeds the token limit.
    /// - [`DiffError::VersionNotFound`] if `parent_id` names a version that is
    ///   not in the store (0.1.x accepted it, and `lineage` then failed later).
    pub fn store(&mut self, version: OutputVersion) -> Result<String, DiffError> {
        if let Some(parent) = &version.parent_id {
            if !self.versions.contains_key(parent) {
                return Err(DiffError::VersionNotFound(parent.clone()));
            }
        }
        let token_estimate = version.content.len() / 4;
        if token_estimate > self.max_output_tokens {
            return Err(DiffError::OutputTooLarge {
                size: token_estimate,
                limit: self.max_output_tokens,
            });
        }
        let id = version.id.clone();
        self.by_address.insert(version.content_address.clone(), id.clone());
        self.versions.insert(id.clone(), version);
        Ok(id)
    }

    /// Retrieves a version by ID.
    ///
    /// # Errors
    /// Returns [`DiffError::VersionNotFound`] if no version with that ID exists.
    pub fn get(&self, id: &str) -> Result<&OutputVersion, DiffError> {
        self.versions.get(id).ok_or_else(|| DiffError::VersionNotFound(id.to_string()))
    }

    /// Retrieves a version by its content address, if present.
    pub fn get_by_address(&self, addr: &str) -> Option<&OutputVersion> {
        self.by_address.get(addr).and_then(|id| self.versions.get(id))
    }

    /// Points a named branch at a version.
    ///
    /// # Errors
    /// Returns [`DiffError::VersionNotFound`] if the version ID does not exist.
    pub fn set_branch(&mut self, branch: impl Into<String>, version_id: impl Into<String>) -> Result<(), DiffError> {
        let vid = version_id.into();
        if !self.versions.contains_key(&vid) {
            return Err(DiffError::VersionNotFound(vid));
        }
        self.branches.insert(branch.into(), vid);
        Ok(())
    }

    /// Returns the head version of a named branch.
    ///
    /// # Errors
    /// - [`DiffError::BranchNotFound`] if the branch does not exist.
    /// - [`DiffError::VersionNotFound`] if the branch head ID is stale.
    pub fn branch_head(&self, branch: &str) -> Result<&OutputVersion, DiffError> {
        let id = self.branches.get(branch)
            .ok_or_else(|| DiffError::BranchNotFound(branch.to_string()))?;
        self.get(id)
    }

    /// Computes a text diff between two stored versions.
    ///
    /// # Errors
    /// Returns [`DiffError::VersionNotFound`] if either ID is missing.
    pub fn diff_versions(&self, from_id: &str, to_id: &str) -> Result<TextDiff, DiffError> {
        let from = self.get(from_id)?;
        let to = self.get(to_id)?;
        Ok(TextDiff::compute(&from.content, &to.content))
    }

    /// Returns the parent version of the given version, or `None` if it is a root.
    ///
    /// # Errors
    /// Returns [`DiffError::VersionNotFound`] if the version ID or its parent ID is missing.
    pub fn rollback(&self, version_id: &str) -> Result<Option<&OutputVersion>, DiffError> {
        let v = self.get(version_id)?;
        match &v.parent_id {
            Some(pid) => Ok(Some(self.get(pid)?)),
            None => Ok(None),
        }
    }

    /// Returns the full ancestor chain starting from `version_id`, oldest last.
    ///
    /// # Errors
    /// Returns [`DiffError::VersionNotFound`] if any version in the chain is missing.
    pub fn lineage(&self, version_id: &str) -> Result<Vec<&OutputVersion>, DiffError> {
        let mut chain = Vec::new();
        let mut current_id = version_id.to_string();
        let mut visited = HashSet::new();
        loop {
            if visited.contains(&current_id) {
                break;
            }
            visited.insert(current_id.clone());
            let v = self.get(&current_id)?;
            chain.push(v);
            match &v.parent_id {
                Some(pid) => current_id = pid.clone(),
                None => break,
            }
        }
        Ok(chain)
    }

    /// Returns the total number of stored versions.
    pub fn version_count(&self) -> usize { self.versions.len() }

    /// Unified diff (`diff -u` format) between two stored versions, with
    /// `context` unchanged lines around each change. Headers are the version ids.
    ///
    /// # Errors
    /// Returns [`DiffError::VersionNotFound`] if either ID is missing.
    pub fn unified_diff(&self, from_id: &str, to_id: &str, context: usize) -> Result<String, DiffError> {
        let from = self.get(from_id)?;
        let to = self.get(to_id)?;
        Ok(crate::diff::unified_diff(&from.content, &to.content, context, from_id, to_id))
    }

    /// Three-way merge of two versions that share an ancestor, for example two
    /// branches that edited the same answer. Needs the `patch` feature.
    ///
    /// # Errors
    /// [`DiffError::VersionNotFound`] for an unknown id, or
    /// [`DiffError::MergeConflict`] (with conflict markers) when the edits overlap.
    #[cfg(feature = "patch")]
    #[cfg_attr(docsrs, doc(cfg(feature = "patch")))]
    pub fn merge(&self, ancestor_id: &str, ours_id: &str, theirs_id: &str) -> Result<String, DiffError> {
        crate::diff::merge3(&self.get(ancestor_id)?.content, &self.get(ours_id)?.content, &self.get(theirs_id)?.content)
    }

    /// Serialize the whole store (versions, branches, limit) to JSON.
    ///
    /// # Errors
    /// Returns [`DiffError::Serialization`] if serialization fails.
    pub fn to_json(&self) -> Result<String, DiffError> {
        Ok(serde_json::to_string(self)?)
    }

    /// Load a store saved with [`to_json`](Self::to_json).
    ///
    /// # Errors
    /// Returns [`DiffError::Serialization`] if the text is not a saved store.
    pub fn from_json(json: &str) -> Result<Self, DiffError> {
        Ok(serde_json::from_str(json)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(content: &str, parent: Option<String>) -> OutputVersion {
        OutputVersion::new(content, "claude-sonnet-4-6", VersionAnnotation::default(), parent)
    }

    #[test]
    fn test_store_and_retrieve_version_by_id() {
        let mut store = VersionStore::new(100_000);
        let ver = v("Hello world", None);
        let id = store.store(ver).unwrap();
        assert!(store.get(&id).is_ok());
    }

    #[test]
    fn test_store_get_nonexistent_id_returns_version_not_found() {
        let store = VersionStore::new(100_000);
        let err = store.get("nonexistent").unwrap_err();
        assert!(matches!(err, DiffError::VersionNotFound(_)));
    }

    #[test]
    fn test_store_output_too_large_returns_error() {
        let mut store = VersionStore::new(1);
        let large = "a ".repeat(1000);
        let err = store.store(v(&large, None)).unwrap_err();
        assert!(matches!(err, DiffError::OutputTooLarge { .. }));
    }

    #[test]
    fn test_store_content_address_dedup_maps_same_content() {
        let mut store = VersionStore::new(100_000);
        let ver1 = v("same content", None);
        let addr = ver1.content_address.clone();
        store.store(ver1).unwrap();
        let ver2 = v("same content", None);
        store.store(ver2).unwrap();
        assert!(store.get_by_address(&addr).is_some());
    }

    #[test]
    fn test_store_set_branch_ok() {
        let mut store = VersionStore::new(100_000);
        let id = store.store(v("content", None)).unwrap();
        assert!(store.set_branch("main", id).is_ok());
    }

    #[test]
    fn test_store_branch_not_found_returns_error() {
        let store = VersionStore::new(100_000);
        let err = store.branch_head("nonexistent").unwrap_err();
        assert!(matches!(err, DiffError::BranchNotFound(_)));
    }

    #[test]
    fn test_store_set_branch_with_invalid_version_returns_version_not_found() {
        let mut store = VersionStore::new(100_000);
        let err = store.set_branch("main", "bad-id").unwrap_err();
        assert!(matches!(err, DiffError::VersionNotFound(_)));
    }

    #[test]
    fn test_store_diff_versions_identical_content_is_identical() {
        let mut store = VersionStore::new(100_000);
        let id1 = store.store(v("same text", None)).unwrap();
        let id2 = store.store(v("same text", None)).unwrap();
        let diff = store.diff_versions(&id1, &id2).unwrap();
        assert!(diff.is_identical());
    }

    #[test]
    fn test_store_rollback_returns_parent_version() {
        let mut store = VersionStore::new(100_000);
        let parent_id = store.store(v("version 1", None)).unwrap();
        let child = v("version 2", Some(parent_id.clone()));
        let child_id = store.store(child).unwrap();
        let parent = store.rollback(&child_id).unwrap().unwrap();
        assert_eq!(parent.id, parent_id);
    }

    #[test]
    fn test_store_rollback_root_returns_none() {
        let mut store = VersionStore::new(100_000);
        let id = store.store(v("root version", None)).unwrap();
        let result = store.rollback(&id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_store_lineage_three_generations_length() {
        let mut store = VersionStore::new(100_000);
        let id1 = store.store(v("v1", None)).unwrap();
        let id2 = store.store(v("v2", Some(id1.clone()))).unwrap();
        let id3 = store.store(v("v3", Some(id2.clone()))).unwrap();
        let lineage = store.lineage(&id3).unwrap();
        assert_eq!(lineage.len(), 3);
    }

    #[test]
    fn test_store_version_count_increments() {
        let mut store = VersionStore::new(100_000);
        assert_eq!(store.version_count(), 0);
        store.store(v("a", None)).unwrap();
        assert_eq!(store.version_count(), 1);
    }

    #[test]
    fn test_store_branch_head_returns_correct_version() {
        let mut store = VersionStore::new(100_000);
        let id = store.store(v("content", None)).unwrap();
        store.set_branch("main", id.clone()).unwrap();
        let head = store.branch_head("main").unwrap();
        assert_eq!(head.id, id);
    }

    #[test]
    fn test_content_address_is_sha256() {
        // Known SHA-256 of "abc".
        assert_eq!(
            content_address("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_store_rejects_unknown_parent() {
        let mut store = VersionStore::new(100_000);
        let err = store.store(v("orphan", Some("missing-id".into()))).unwrap_err();
        assert!(matches!(err, DiffError::VersionNotFound(id) if id == "missing-id"));
        assert_eq!(store.version_count(), 0);
    }

    #[test]
    fn test_store_json_roundtrip() {
        let mut store = VersionStore::new(100_000);
        let id1 = store.store(v("first", None)).unwrap();
        let id2 = store.store(v("second", Some(id1.clone()))).unwrap();
        store.set_branch("main", id2.clone()).unwrap();
        let restored = VersionStore::from_json(&store.to_json().unwrap()).unwrap();
        assert_eq!(restored.version_count(), 2);
        assert_eq!(restored.branch_head("main").unwrap().id, id2);
        assert_eq!(restored.lineage(&id2).unwrap().len(), 2);
        assert!(VersionStore::from_json("{}").is_err());
    }

    #[test]
    fn test_store_unified_diff() {
        let mut store = VersionStore::new(100_000);
        let a = store.store(v("one\ntwo\n", None)).unwrap();
        let b = store.store(v("one\n2\n", Some(a.clone()))).unwrap();
        let u = store.unified_diff(&a, &b, 3).unwrap();
        assert!(u.contains("-two") && u.contains("+2"));
        assert!(store.unified_diff(&a, "nope", 3).is_err());
    }
}
