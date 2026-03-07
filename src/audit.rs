// SPDX-License-Identifier: MIT
//! Append-only audit log for compliance and legal review of output changes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single audit event recorded in the log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditEvent {
    /// A new version was created and stored.
    VersionCreated { version_id: String, model: String, timestamp: DateTime<Utc> },
    /// A branch head was updated.
    BranchCreated { branch: String, head_version_id: String, timestamp: DateTime<Utc> },
    /// The store was rolled back from one version to another.
    Rollback { from_version_id: String, to_version_id: String, timestamp: DateTime<Utc> },
    /// A diff was computed between two versions.
    DiffComputed { from_id: String, to_id: String, similarity: f64, timestamp: DateTime<Utc> },
}

impl AuditEvent {
    /// Returns a human-readable kind label for the event.
    pub fn kind(&self) -> &'static str {
        match self {
            AuditEvent::VersionCreated { .. } => "VersionCreated",
            AuditEvent::BranchCreated { .. } => "BranchCreated",
            AuditEvent::Rollback { .. } => "Rollback",
            AuditEvent::DiffComputed { .. } => "DiffComputed",
        }
    }

    /// Returns the UTC timestamp of the event.
    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            AuditEvent::VersionCreated { timestamp, .. }
            | AuditEvent::BranchCreated { timestamp, .. }
            | AuditEvent::Rollback { timestamp, .. }
            | AuditEvent::DiffComputed { timestamp, .. } => *timestamp,
        }
    }
}

/// An append-only audit log. Events are never removed once recorded.
pub struct AuditLog {
    events: Vec<AuditEvent>,
}

impl AuditLog {
    /// Creates a new, empty audit log.
    pub fn new() -> Self { Self { events: Vec::new() } }

    /// Appends an event to the log.
    pub fn record(&mut self, event: AuditEvent) { self.events.push(event); }

    /// Returns all recorded events in insertion order.
    pub fn events(&self) -> &[AuditEvent] { &self.events }

    /// Returns the number of recorded events.
    pub fn len(&self) -> usize { self.events.len() }

    /// Returns `true` if no events have been recorded.
    pub fn is_empty(&self) -> bool { self.events.is_empty() }

    /// Serializes the entire log to a JSON string.
    ///
    /// # Errors
    /// Returns a [`serde_json::Error`] if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.events)
    }

    /// Returns all events whose kind label matches `kind`.
    pub fn events_of_kind(&self, kind: &str) -> Vec<&AuditEvent> {
        self.events.iter().filter(|e| e.kind() == kind).collect()
    }
}

impl Default for AuditLog {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_log_record_increments_len() {
        let mut log = AuditLog::new();
        log.record(AuditEvent::VersionCreated {
            version_id: "v1".into(),
            model: "claude".into(),
            timestamp: Utc::now(),
        });
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn test_audit_log_is_empty_initially() {
        let log = AuditLog::new();
        assert!(log.is_empty());
    }

    #[test]
    fn test_audit_log_filter_by_kind_version_created() {
        let mut log = AuditLog::new();
        log.record(AuditEvent::VersionCreated {
            version_id: "v1".into(),
            model: "m".into(),
            timestamp: Utc::now(),
        });
        log.record(AuditEvent::Rollback {
            from_version_id: "v2".into(),
            to_version_id: "v1".into(),
            timestamp: Utc::now(),
        });
        assert_eq!(log.events_of_kind("VersionCreated").len(), 1);
        assert_eq!(log.events_of_kind("Rollback").len(), 1);
    }

    #[test]
    fn test_audit_log_to_json_contains_event_kind() {
        let mut log = AuditLog::new();
        log.record(AuditEvent::DiffComputed {
            from_id: "a".into(),
            to_id: "b".into(),
            similarity: 0.8,
            timestamp: Utc::now(),
        });
        let json = log.to_json().unwrap();
        assert!(json.contains("DiffComputed"));
    }

    #[test]
    fn test_audit_log_grows_monotonically() {
        let mut log = AuditLog::new();
        for i in 0..5u32 {
            log.record(AuditEvent::VersionCreated {
                version_id: format!("v{i}"),
                model: "m".into(),
                timestamp: Utc::now(),
            });
            assert_eq!(log.len(), (i + 1) as usize);
        }
    }

    #[test]
    fn test_audit_event_kind_labels_correct() {
        assert_eq!(AuditEvent::VersionCreated { version_id: "".into(), model: "".into(), timestamp: Utc::now() }.kind(), "VersionCreated");
        assert_eq!(AuditEvent::BranchCreated { branch: "".into(), head_version_id: "".into(), timestamp: Utc::now() }.kind(), "BranchCreated");
        assert_eq!(AuditEvent::Rollback { from_version_id: "".into(), to_version_id: "".into(), timestamp: Utc::now() }.kind(), "Rollback");
        assert_eq!(AuditEvent::DiffComputed { from_id: "".into(), to_id: "".into(), similarity: 0.0, timestamp: Utc::now() }.kind(), "DiffComputed");
    }

    #[test]
    fn test_audit_log_events_of_kind_empty_when_no_match() {
        let mut log = AuditLog::new();
        log.record(AuditEvent::VersionCreated { version_id: "v1".into(), model: "m".into(), timestamp: Utc::now() });
        assert!(log.events_of_kind("Rollback").is_empty());
    }
}
