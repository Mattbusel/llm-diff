// SPDX-License-Identifier: MIT
use chrono::Utc;
use llm_diff::diff::json_diff;
use llm_diff::{AuditEvent, AuditLog, DiffError, OutputVersion, VersionAnnotation, VersionStore};

#[test]
fn test_full_versioning_workflow_with_audit_trail() {
    let mut store = VersionStore::new(100_000);
    let mut log = AuditLog::new();

    let v1 = OutputVersion::new(
        "Initial output from the model.",
        "claude-sonnet-4-6",
        VersionAnnotation::default(),
        None,
    );
    let id1 = store.store(v1).unwrap();
    log.record(AuditEvent::VersionCreated {
        version_id: id1.clone(),
        model: "claude-sonnet-4-6".into(),
        timestamp: Utc::now(),
    });

    let v2 = OutputVersion::new(
        "Improved output from the model with more detail.",
        "claude-sonnet-4-6",
        VersionAnnotation { prompt_changed: true, ..Default::default() },
        Some(id1.clone()),
    );
    let id2 = store.store(v2).unwrap();
    log.record(AuditEvent::VersionCreated {
        version_id: id2.clone(),
        model: "claude-sonnet-4-6".into(),
        timestamp: Utc::now(),
    });

    let diff = store.diff_versions(&id1, &id2).unwrap();
    assert!(!diff.is_identical());
    log.record(AuditEvent::DiffComputed {
        from_id: id1.clone(),
        to_id: id2.clone(),
        similarity: diff.similarity,
        timestamp: Utc::now(),
    });

    let parent = store.rollback(&id2).unwrap().unwrap();
    assert_eq!(parent.id, id1);
    log.record(AuditEvent::Rollback {
        from_version_id: id2,
        to_version_id: id1,
        timestamp: Utc::now(),
    });

    assert_eq!(log.len(), 4);
}

#[test]
fn test_json_diff_detects_model_output_change() {
    let old = r#"{"answer": "Paris", "confidence": 0.9}"#;
    let new = r#"{"answer": "London", "confidence": 0.7, "source": "web"}"#;
    let ops = json_diff(old, new).unwrap();
    assert!(ops.len() > 1);
}

#[test]
fn test_lineage_three_generations() {
    let mut store = VersionStore::new(100_000);
    let id1 = store.store(OutputVersion::new("gen1", "m", VersionAnnotation::default(), None)).unwrap();
    let id2 = store.store(OutputVersion::new("gen2", "m", VersionAnnotation::default(), Some(id1))).unwrap();
    let id3 = store.store(OutputVersion::new("gen3", "m", VersionAnnotation::default(), Some(id2))).unwrap();
    let lineage = store.lineage(&id3).unwrap();
    assert_eq!(lineage.len(), 3);
}

#[test]
fn test_branch_workflow_set_and_retrieve() {
    let mut store = VersionStore::new(100_000);
    let id = store.store(OutputVersion::new("main content", "m", VersionAnnotation::default(), None)).unwrap();
    store.set_branch("main", id.clone()).unwrap();
    let head = store.branch_head("main").unwrap();
    assert_eq!(head.id, id);
}

#[test]
fn test_version_not_found_error_propagates() {
    let store = VersionStore::new(100_000);
    let err = store.get("does-not-exist").unwrap_err();
    assert!(matches!(err, DiffError::VersionNotFound(_)));
}

#[test]
fn test_audit_log_json_serialization_roundtrip() {
    let mut log = AuditLog::new();
    log.record(AuditEvent::BranchCreated {
        branch: "main".into(),
        head_version_id: "abc".into(),
        timestamp: Utc::now(),
    });
    let json = log.to_json().unwrap();
    assert!(json.contains("BranchCreated"));
}
