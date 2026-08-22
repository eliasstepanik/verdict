use std::collections::HashMap;

use chrono::Utc;

use super::types::{AuditEntry, AuditEvent, CallTreeNode, CallTreeStatus};

/// Build a call tree from audit log entries
///
/// This function traverses delegation events and reconstructs the hierarchical
/// structure of agent calls, matching parent-child relationships by task_id.
pub fn call_tree_from_audit_log(entries: &[AuditEntry]) -> Vec<CallTreeNode> {
    // Build: child_agent_name -> parent_agent_name
    let mut parent_of: HashMap<String, String> = HashMap::new();
    // Build: agent_name -> (started_at, completed_at, status)
    let mut node_data: HashMap<String, (chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>, CallTreeStatus)> =
        HashMap::new();

    for entry in entries {
        match &entry.event {
            AuditEvent::DelegationStarted {
                parent_agent,
                child_agent,
                ..
            } => {
                parent_of.insert(child_agent.clone(), parent_agent.clone());
                node_data.entry(child_agent.clone()).or_insert((
                    entry.timestamp,
                    None,
                    CallTreeStatus::Running,
                ));
                node_data.entry(parent_agent.clone()).or_insert((
                    entry.timestamp,
                    None,
                    CallTreeStatus::Running,
                ));
            }
            AuditEvent::DelegationCompleted { child_agent, .. } => {
                if let Some(data) = node_data.get_mut(child_agent) {
                    data.1 = Some(entry.timestamp);
                    data.2 = CallTreeStatus::Completed;
                }
            }
            AuditEvent::DelegationFailed {
                child_agent,
                reason,
                ..
            } => {
                if let Some(data) = node_data.get_mut(child_agent) {
                    data.1 = Some(entry.timestamp);
                    data.2 = CallTreeStatus::Failed {
                        reason: reason.clone(),
                    };
                }
            }
            _ => {}
        }
    }

    // Find roots: agents that are NOT in parent_of values
    let all_children: std::collections::HashSet<&str> =
        parent_of.keys().map(|s| s.as_str()).collect();

    fn build_node(
        agent_name: &str,
        node_data: &HashMap<String, (chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>, CallTreeStatus)>,
        parent_of: &HashMap<String, String>,
    ) -> CallTreeNode {
        let (started, completed, status) = node_data.get(agent_name).cloned().unwrap_or((
            Utc::now(),
            None,
            CallTreeStatus::Running,
        ));

        let children: Vec<CallTreeNode> = parent_of
            .iter()
            .filter(|(_, parent)| *parent == agent_name)
            .map(|(child, _)| build_node(child, node_data, parent_of))
            .collect();

        CallTreeNode {
            agent_name: agent_name.to_string(),
            depth: 0, // set by caller if needed
            started_at: started,
            completed_at: completed,
            status,
            children,
        }
    }

    node_data
        .keys()
        .filter(|name| !all_children.contains(name.as_str()))
        .map(|root| build_node(root, &node_data, &parent_of))
        .collect()
}
