// SPDX-License-Identifier: MIT
//! Content-addressed version store for LLM outputs. Needs the `store`
//! feature (on by default).
//!
//! Every [`OutputVersion`] carries the SHA-256 of its text. Storing the same
//! text twice returns the id of the version that is already there, so a
//! store never holds two copies of one output.
//!
//! ```
//! use llm_diff::{OutputVersion, VersionAnnotation, VersionStore};
//!
//! let mut store = VersionStore::new(64 * 1024); // refuse outputs over 64 KiB
//! let v1 = store.store(OutputVersion::new("The answer is Paris.", "gpt-4o", VersionAnnotation::default(), None))?;
//! let why = VersionAnnotation { prompt_changed: true, note: Some("added 'be brief'".into()), ..Default::default() };
//! let v2 = store.store(OutputVersion::new("Paris.", "gpt-4o", why, Some(v1.clone())))?;
//! store.set_branch("main", v2.as_str())?;
//!
//! // Same text again: no new version, the existing id comes back.
//! let again = store.store(OutputVersion::new("Paris.", "gpt-4o", VersionAnnotation::default(), None))?;
//! assert_eq!(again, v2);
//! assert_eq!(store.version_count(), 2);
//!
//! assert_eq!(store.lineage(&v2)?.len(), 2);
//! let saved = store.to_json()?;
//! assert_eq!(llm_diff::VersionStore::from_json(&saved)?.version_count(), 2);
//! # Ok::<(), llm_diff::DiffError>(())
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};
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
///
/// Build it with struct update syntax so that new fields do not break your
/// code: `VersionAnnotation { prompt_changed: true, ..Default::default() }`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
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
    /// Anything else worth keeping with the version: prompt id, seed, top_p,
    /// eval score, tool versions. Omitted from JSON when empty.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, serde_json::Value>,
}


/// A stored version of an LLM output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct OutputVersion {
    /// Unique UUID for this version.
    pub id: String,
    /// SHA-256 of the content (64 lowercase hex characters). The store
    /// recomputes it on [`VersionStore::store`] and uses it to deduplicate.
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

/// Content-addressed version store with branch and lineage support.
///
/// The whole store serializes with serde (for example to JSON with
/// [`to_json`](Self::to_json)), so it can be saved and loaded between runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct VersionStore {
    versions: HashMap<String, OutputVersion>,
    by_address: HashMap<String, String>,
    branches: HashMap<String, String>,
    /// Stores saved by 0.2 call this `max_output_tokens`; they load with that
    /// number read as bytes.
    #[serde(alias = "max_output_tokens")]
    max_bytes: usize,
}

impl VersionStore {
    /// Creates a new store that refuses outputs longer than `max_bytes` bytes
    /// of UTF-8 text.
    ///
    /// Until 0.2 this limit was in "tokens", estimated as bytes / 4, which is
    /// wrong for most non-English text. If you need a token limit, count
    /// tokens with your model's tokenizer before calling [`store`](Self::store).
    pub fn new(max_bytes: usize) -> Self {
        Self {
            versions: HashMap::new(),
            by_address: HashMap::new(),
            branches: HashMap::new(),
            max_bytes,
        }
    }

    /// The size limit in bytes given to [`new`](Self::new).
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// Stores a version and returns its ID.
    ///
    /// Deduplicates on content: if a version with the same text is already
    /// stored, nothing is added and that version's ID is returned, so the
    /// model, annotation and parent of `version` are dropped. Check
    /// [`get_by_address`](Self::get_by_address) first if you need to know.
    /// The content address is recomputed from the text, so an edited
    /// `content` with a stale `content_address` is still filed correctly.
    ///
    /// # Errors
    /// - [`DiffError::VersionNotFound`] if `parent_id` names a version that is
    ///   not in the store.
    /// - [`DiffError::OutputTooLarge`] if the content is longer than the
    ///   store's byte limit.
    /// - [`DiffError::DuplicateVersionId`] if a version with the same ID but
    ///   different text is already stored.
    pub fn store(&mut self, mut version: OutputVersion) -> Result<String, DiffError> {
        if let Some(parent) = &version.parent_id {
            if !self.versions.contains_key(parent) {
                return Err(DiffError::VersionNotFound(parent.clone()));
            }
        }
        let size = version.content.len();
        if size > self.max_bytes {
            return Err(DiffError::OutputTooLarge { size, limit: self.max_bytes });
        }
        version.content_address = content_address(&version.content);
        if let Some(existing) = self.by_address.get(&version.content_address) {
            return Ok(existing.clone());
        }
        if self.versions.contains_key(&version.id) {
            return Err(DiffError::DuplicateVersionId(version.id));
        }
        let id = version.id.clone();
        self.by_address.insert(version.content_address.clone(), id.clone());
        self.versions.insert(id.clone(), version);
        Ok(id)
    }

    /// All stored versions, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = &OutputVersion> + '_ {
        self.versions.values()
    }

    /// Branch names and the version ID each one points at, in no particular order.
    pub fn branches(&self) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.branches.iter().map(|(b, id)| (b.as_str(), id.as_str()))
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

    /// Load a store saved with [`to_json`](Self::to_json), and check that it
    /// is consistent: every version is filed under its own ID, every parent
    /// exists, and every branch and address entry points at a stored version.
    ///
    /// Stores saved by 0.2 load. Duplicate copies of the same text stay, and
    /// the address index keeps pointing at the copy it pointed at. Each
    /// version's `content_address` is recomputed from its text, so a version
    /// whose `content` was edited after [`OutputVersion::new`] (0.2 saved its
    /// stale address) is re-filed under the SHA-256 of what it actually holds.
    ///
    /// # Errors
    /// - [`DiffError::Serialization`] if the text is not a saved store.
    /// - [`DiffError::InvalidStore`] if the store is inconsistent.
    pub fn from_json(json: &str) -> Result<Self, DiffError> {
        let mut store: Self = serde_json::from_str(json)?;
        store.validate()?;
        Ok(store)
    }

    /// Checks the invariants that [`store`](Self::store) maintains and
    /// repairs what 0.2 could legitimately leave behind.
    ///
    /// Rejected: a version filed under another id, a parent, branch or
    /// address entry that names a version not in the store.
    ///
    /// Repaired: 0.2 never recomputed the content address, so a version whose
    /// public `content` field was edited after [`OutputVersion::new`] was saved
    /// with a stale `content_address`. The address is recomputed from the text
    /// and the version is re-filed under it; index entries that no longer
    /// match their version's address are dropped, and missing entries are
    /// added (the newest version wins when 0.2 kept several copies).
    fn validate(&mut self) -> Result<(), DiffError> {
        let bad = |msg: String| Err(DiffError::InvalidStore(msg));
        for (key, v) in &self.versions {
            if key != &v.id {
                return bad(format!("version filed under '{key}' has id '{}'", v.id));
            }
            if let Some(parent) = &v.parent_id {
                if !self.versions.contains_key(parent) {
                    return bad(format!("version '{key}' has parent '{parent}', which is not in the store"));
                }
            }
        }
        for (addr, id) in &self.by_address {
            if !self.versions.contains_key(id) {
                return bad(format!("address '{addr}' points at missing version '{id}'"));
            }
        }
        for (branch, id) in &self.branches {
            if !self.versions.contains_key(id) {
                return bad(format!("branch '{branch}' points at missing version '{id}'"));
            }
        }
        for v in self.versions.values_mut() {
            let addr = content_address(&v.content);
            if v.content_address != addr {
                v.content_address = addr;
            }
        }
        let versions = &self.versions;
        self.by_address.retain(|addr, id| versions.get(id).is_some_and(|v| &v.content_address == addr));
        let mut newest_first: Vec<&OutputVersion> = versions.values().collect();
        newest_first.sort_by(|a, b| (b.created_at, &b.id).cmp(&(a.created_at, &a.id)));
        for v in newest_first {
            self.by_address.entry(v.content_address.clone()).or_insert_with(|| v.id.clone());
        }
        Ok(())
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
        let mut store = VersionStore::new(10);
        let err = store.store(v("eleven byte", None)).unwrap_err();
        assert!(matches!(err, DiffError::OutputTooLarge { size: 11, limit: 10 }));
        // the limit is bytes, inclusive: 10 bytes fit, and so do 3 CJK chars (9 bytes)
        assert!(store.store(v("ten bytes!", None)).is_ok());
        assert!(store.store(v("猫猫猫", None)).is_ok());
        assert!(store.store(v("猫猫猫猫", None)).is_err());
        assert_eq!(store.max_bytes(), 10);
    }

    #[test]
    fn test_store_content_address_dedup_maps_same_content() {
        let mut store = VersionStore::new(100_000);
        let ver1 = v("same content", None);
        let addr = ver1.content_address.clone();
        let id1 = store.store(ver1).unwrap();
        let id2 = store.store(v("same content", None)).unwrap();
        assert_eq!(id1, id2, "same text must come back as the same version");
        assert_eq!(store.version_count(), 1);
        assert_eq!(store.get_by_address(&addr).unwrap().id, id1);
    }

    #[test]
    fn test_store_recomputes_stale_content_address() {
        let mut store = VersionStore::new(100_000);
        let mut ver = v("original", None);
        ver.content = "edited".into();
        let id = store.store(ver).unwrap();
        assert_eq!(store.get(&id).unwrap().content_address, content_address("edited"));
        assert!(store.get_by_address(&content_address("edited")).is_some());
        assert!(store.get_by_address(&content_address("original")).is_none());
    }

    #[test]
    fn test_store_rejects_reused_id_with_other_content() {
        let mut store = VersionStore::new(100_000);
        let ver = v("first", None);
        let mut copy = ver.clone();
        let id = store.store(ver).unwrap();
        // same id, same text: deduplicated
        assert_eq!(store.store(copy.clone()).unwrap(), id);
        // same id, other text: refused instead of silently overwriting
        copy.content = "second".into();
        let err = store.store(copy).unwrap_err();
        assert!(matches!(err, DiffError::DuplicateVersionId(ref d) if *d == id));
        assert_eq!(store.get(&id).unwrap().content, "first");
    }

    #[test]
    fn test_from_json_rejects_dangling_references() {
        let mut store = VersionStore::new(100_000);
        let id = store.store(v("x", None)).unwrap();
        store.set_branch("main", id.clone()).unwrap();
        let good: serde_json::Value = serde_json::from_str(&store.to_json().unwrap()).unwrap();

        let mut dangling_branch = good.clone();
        dangling_branch["branches"]["main"] = "missing".into();
        let mut dangling_address = good.clone();
        dangling_address["by_address"] = serde_json::json!({ content_address("x"): "missing" });
        let mut misfiled = good.clone();
        misfiled["versions"][&id]["id"] = "other".into();
        let mut orphan = good.clone();
        orphan["versions"][&id]["parent_id"] = "missing".into();
        for bad in [dangling_branch, dangling_address, misfiled, orphan] {
            let err = VersionStore::from_json(&bad.to_string()).unwrap_err();
            assert!(matches!(err, DiffError::InvalidStore(_)), "{err}");
        }

        // 0.2 saved a stale content_address when `content` was edited after
        // OutputVersion::new: the store re-files it instead of refusing to load
        let mut edited_in_0_2 = good.clone();
        edited_in_0_2["versions"][&id]["content"] = "not x".into();
        let restored = VersionStore::from_json(&edited_in_0_2.to_string()).unwrap();
        assert_eq!(restored.get(&id).unwrap().content_address, content_address("not x"));
        assert_eq!(restored.get_by_address(&content_address("not x")).unwrap().id, id);
        assert!(restored.get_by_address(&content_address("x")).is_none());

        // a missing address entry is rebuilt rather than rejected
        let mut no_index = good;
        no_index["by_address"] = serde_json::json!({});
        let restored = VersionStore::from_json(&no_index.to_string()).unwrap();
        assert_eq!(restored.get_by_address(&content_address("x")).unwrap().id, id);
    }

    #[test]
    fn test_from_json_reads_0_2_field_name() {
        let json = r#"{"versions":{},"by_address":{},"branches":{},"max_output_tokens":8000}"#;
        assert_eq!(VersionStore::from_json(json).unwrap().max_bytes(), 8000);
    }

    #[test]
    fn test_annotation_metadata_roundtrip() {
        let mut ann = VersionAnnotation { model_changed: true, ..Default::default() };
        ann.metadata.insert("seed".into(), 42.into());
        let json = serde_json::to_string(&ann).unwrap();
        assert_eq!(serde_json::from_str::<VersionAnnotation>(&json).unwrap(), ann);
        // empty metadata is left out, and old JSON without it still loads
        let plain = serde_json::to_string(&VersionAnnotation::default()).unwrap();
        assert!(!plain.contains("metadata"));
        let old = r#"{"prompt_changed":true,"model_changed":false,"temperature_changed":false,"note":null}"#;
        assert!(serde_json::from_str::<VersionAnnotation>(old).unwrap().prompt_changed);
    }

    #[test]
    fn test_iter_and_branches() {
        let mut store = VersionStore::new(100_000);
        let a = store.store(v("a", None)).unwrap();
        let b = store.store(v("b", Some(a.clone()))).unwrap();
        store.set_branch("main", b.clone()).unwrap();
        let mut ids: Vec<&str> = store.iter().map(|v| v.id.as_str()).collect();
        ids.sort_unstable();
        let mut expected = [a.as_str(), b.as_str()];
        expected.sort_unstable();
        assert_eq!(ids, expected);
        assert_eq!(store.branches().collect::<Vec<_>>(), [("main", b.as_str())]);
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
        let id2 = store.store(v("other text", None)).unwrap();
        assert!(store.diff_versions(&id1, &id1).unwrap().is_identical());
        assert!(!store.diff_versions(&id1, &id2).unwrap().is_identical());
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
