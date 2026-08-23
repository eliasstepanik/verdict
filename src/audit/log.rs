use chrono::Utc;
use serde_json::{json, Value};
use std::collections::VecDeque;

use super::types::AuditEntry;
use super::json_encode::event_to_json;
use super::json_decode::event_from_json;

/// Audit log for pipeline execution with bounded FIFO eviction
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditLog {
    entries: VecDeque<AuditEntry>,
    #[serde(default = "default_max_entries")]
    max_entries: usize,
}

/// Default maximum entries in audit log (10,000)
fn default_max_entries() -> usize {
    10_000
}

impl AuditLog {
    /// Create a new audit log with default max_entries
    pub fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries: default_max_entries(),
        }
    }

    /// Create a new audit log with a custom max_entries cap
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries,
        }
    }

    /// Append an entry to the log, evicting the oldest entry if at capacity
    pub fn append(&mut self, entry: AuditEntry) {
        if self.entries.len() >= self.max_entries {
            // Evict oldest (front)
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Get all entries from front to back (non-contiguous support via iteration)
    /// NOTE: VecDeque returns entries in 2 slices due to ring buffer; for the audit log API,
    /// callers iterate via the full entries iterator or use this method to convert to Vec
    pub fn entries(&self) -> Vec<AuditEntry> {
        self.entries.iter().cloned().collect()
    }

    /// Get entries as slices (returns (front, back) tuple for ring buffer layout)
    /// This is used internally where the full contiguous slice is not required
    pub fn entries_slices(&self) -> (&[AuditEntry], &[AuditEntry]) {
        self.entries.as_slices()
    }

    /// Get max entries cap
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Get current entry count
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if log is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        let entries: Vec<Value> = self
            .entries
            .iter()
            .map(|entry| {
                json!({
                    "timestamp": entry.timestamp.to_rfc3339(),
                    "pipeline_name": entry.pipeline_name,
                    "step_name": entry.step_name,
                    "event": event_to_json(&entry.event),
                })
            })
            .collect();

        let log_json = json!({
            "entries": entries
        });

        serde_json::to_string(&log_json)
    }

    /// Save audit log to a file as JSON
    pub fn save_to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let json_str = self.to_json()?;
        std::fs::write(path, json_str)?;
        Ok(())
    }

    /// Load audit log from a JSON file with eviction to max_entries if oversized
    pub fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let json_str = std::fs::read_to_string(path)?;
        let log_json: Value = serde_json::from_str(&json_str)?;

        let mut entries = VecDeque::new();
        let max_entries = default_max_entries();

        if let Some(entries_array) = log_json.get("entries").and_then(|v| v.as_array()) {
            for entry_json in entries_array {
                if let (
                    Some(timestamp_str),
                    Some(pipeline_name),
                    Some(step_name),
                    Some(event_obj),
                ) = (
                    entry_json.get("timestamp").and_then(|v| v.as_str()),
                    entry_json.get("pipeline_name").and_then(|v| v.as_str()),
                    entry_json.get("step_name").and_then(|v| v.as_str()),
                    entry_json.get("event").and_then(|v| v.as_object()),
                ) {
                    let timestamp =
                        chrono::DateTime::parse_from_rfc3339(timestamp_str)?.with_timezone(&Utc);

                    if let Some(event) = event_from_json(event_obj) {
                        // Apply eviction on load if we're at capacity
                        if entries.len() >= max_entries {
                            entries.pop_front();
                        }
                        entries.push_back(AuditEntry {
                            timestamp,
                            pipeline_name: pipeline_name.to_string(),
                            step_name: step_name.to_string(),
                            event,
                        });
                    }
                }
            }
        }

        Ok(AuditLog { entries, max_entries })
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::AuditEvent;

    #[test]
    fn test_audit_log_bounded_fifo_eviction() {
        let mut log = AuditLog::with_capacity(100);
        
        for i in 0..250 {
            let entry = AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: format!("pipeline_{}", i),
                step_name: format!("step_{}", i),
                event: AuditEvent::StepStarted,
            };
            log.append(entry);
        }
        
        assert_eq!(log.len(), 100, "Expected 100 entries after appending 250");
        let entries = log.entries();
        assert_eq!(entries.len(), 100);
        assert_eq!(entries[0].pipeline_name, "pipeline_150", 
                   "First entry should be from index 150 (oldest retained)");
        assert_eq!(entries[99].pipeline_name, "pipeline_249",
                   "Last entry should be from index 249 (most recent)");
        assert_eq!(entries[50].pipeline_name, "pipeline_200",
                   "Middle entry should be from index 200");
    }

    #[test]
    fn test_audit_log_default_capacity() {
        let log = AuditLog::new();
        assert_eq!(log.max_entries(), default_max_entries());
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn test_audit_log_small_capacity() {
        let mut log = AuditLog::with_capacity(5);
        
        for i in 0..10 {
            let entry = AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: format!("test_{}", i),
                step_name: "step".to_string(),
                event: AuditEvent::StepCompleted { verdict_passed: true },
            };
            log.append(entry);
        }
        
        assert_eq!(log.len(), 5);
        let entries = log.entries();
        assert_eq!(entries[0].pipeline_name, "test_5");
        assert_eq!(entries[4].pipeline_name, "test_9");
    }

    #[test]
    fn test_audit_log_serde_roundtrip_preserves_entries() {
        // Regression test: #[serde(skip)] on entries field caused silent data loss
        // This test verifies that entries survive JSON serialization/deserialization
        let mut log = AuditLog::with_capacity(100);

        // Append 3 distinct entries
        let now = Utc::now();
        let entry1 = AuditEntry {
            timestamp: now,
            pipeline_name: "pipe_one".to_string(),
            step_name: "step_alpha".to_string(),
            event: AuditEvent::StepStarted,
        };
        let entry2 = AuditEntry {
            timestamp: now + chrono::Duration::seconds(1),
            pipeline_name: "pipe_two".to_string(),
            step_name: "step_beta".to_string(),
            event: AuditEvent::StepCompleted { verdict_passed: true },
        };
        let entry3 = AuditEntry {
            timestamp: now + chrono::Duration::seconds(2),
            pipeline_name: "pipe_three".to_string(),
            step_name: "step_gamma".to_string(),
            event: AuditEvent::StepFailed { error: "oops".to_string() },
        };

        log.append(entry1.clone());
        log.append(entry2.clone());
        log.append(entry3.clone());

        // Serialize to JSON
        let json_str = serde_json::to_string(&log)
            .expect("Failed to serialize AuditLog");

        // Deserialize back
        let restored_log: AuditLog = serde_json::from_str(&json_str)
            .expect("Failed to deserialize AuditLog");

        // Verify entries count and field-by-field content match
        assert_eq!(restored_log.len(), 3, "Expected 3 entries after round-trip");

        let restored_entries = restored_log.entries();
        
        // Entry 1
        assert_eq!(restored_entries[0].pipeline_name, "pipe_one");
        assert_eq!(restored_entries[0].step_name, "step_alpha");
        assert_eq!(restored_entries[0].timestamp, entry1.timestamp);
        match &restored_entries[0].event {
            AuditEvent::StepStarted => {},
            _ => panic!("Entry 1 event mismatch"),
        }

        // Entry 2
        assert_eq!(restored_entries[1].pipeline_name, "pipe_two");
        assert_eq!(restored_entries[1].step_name, "step_beta");
        assert_eq!(restored_entries[1].timestamp, entry2.timestamp);
        match &restored_entries[1].event {
            AuditEvent::StepCompleted { verdict_passed: true } => {},
            _ => panic!("Entry 2 event mismatch"),
        }

        // Entry 3
        assert_eq!(restored_entries[2].pipeline_name, "pipe_three");
        assert_eq!(restored_entries[2].step_name, "step_gamma");
        assert_eq!(restored_entries[2].timestamp, entry3.timestamp);
        match &restored_entries[2].event {
            AuditEvent::StepFailed { error } if error == "oops" => {},
            _ => panic!("Entry 3 event mismatch"),
        }
    }
}
