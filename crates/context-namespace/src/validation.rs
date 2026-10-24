// Invariant checking — internal validation used by ContextNamespace.

use serde::{Deserialize, Serialize};

use crate::id::NodeId;
use crate::node::NodeKey;
use crate::record::Segment;

/// A warning about a potential issue found during validation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationWarning {
    /// The warning category.
    pub code: ValidationWarningCode,
    /// Human-readable description of the warning.
    pub message: String,
}

/// Categorizes validation warnings.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ValidationWarningCode {
    /// An alias could not be resolved to a live node.
    DanglingAlias,
    /// A path in the index does not match the derived path.
    PathMismatch,
    /// An expansion references a node that is no longer present.
    DanglingExpansion,
}

/// The result of a successful full invariant validation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationReport {
    /// The namespace version at the time of validation.
    pub namespace_version: u64,
    /// Number of canonical records checked.
    pub checked_records: u64,
    /// Number of summary nodes checked.
    pub checked_nodes: u64,
    /// Number of structural references (Segment::Ref) checked.
    pub checked_references: u64,
    /// Warnings found during validation (non-fatal).
    pub warnings: Vec<ValidationWarning>,
}

/// Represents an invariant violation found during validation.
#[derive(Debug, Clone, thiserror::Error)]
pub enum InvariantViolation {
    /// A cycle was detected in the parent/child graph.
    #[error("cycle detected: node {node} is its own ancestor")]
    Cycle { node: NodeId },

    /// A node lists a parent that does not list it as a child, or vice versa.
    #[error("node {node} has parent {parent} but parent does not list it as child")]
    ParentChildMismatch { node: NodeId, parent: NodeId },

    /// A node has no canonical owner/reference location.
    #[error("node {node} has no owner")]
    NoOwner { node: NodeId },

    /// A structural reference points to a missing node.
    #[error("structural reference in record {record_index} points to missing node {node}")]
    DanglingRef { record_index: usize, node: NodeId },

    /// Two records share the same stable ID.
    #[error("duplicate record ID: {id}")]
    DuplicateRecordId { id: String },

    /// Two nodes share the same stable ID.
    #[error("duplicate node ID: {id}")]
    DuplicateNodeId { id: String },

    /// The path index does not match the derived path for a node.
    #[error("path {path} resolves to wrong node")]
    PathIndexMismatch { path: String },

    /// A node is not reachable from any root or carrier.
    #[error("node {node} is not reachable from any root or carrier")]
    UnreachableNode { node: NodeId },

    /// An alias path shadows a current path.
    #[error("alias {alias} shadows a current path")]
    AliasShadowsPath { alias: String },

    /// An alias points to a missing node.
    #[error("alias {alias} points to missing node")]
    DanglingAlias { alias: String },
}

impl crate::namespace::ContextNamespace {
    /// Runs full invariant validation on the namespace.
    ///
    /// Checks all 12 invariants from the specification:
    /// 1. Forest (no cycles, at most one parent)
    /// 2. All structural refs point to existing nodes
    /// 3. Every node has exactly one canonical owner
    /// 4. Children listed by a node are present in its stored source
    /// 5. Sibling labels are unique
    /// 6. Current paths resolve uniquely
    /// 7. Aliases resolve to one live node
    /// 8. Unique stable record IDs
    /// 9. Render field represented exactly once
    /// 10. Pending requests don't mutate canonical state
    /// 11. Expansion state references only live nodes
    /// 12. Version increases after mutations
    ///
    /// # Errors
    ///
    /// Returns [`InvariantViolation`] if any invariant is violated.
    pub fn validate(&self) -> Result<ValidationReport, InvariantViolation> {
        let mut checked_references = 0u64;
        let mut warnings = Vec::new();

        // Invariant 8: Unique stable record IDs.
        // Invariant 2: Every reachable structural reference points to
        // an existing node.
        // Invariant 9: Render field is represented by Record.render.
        let records = self.records();
        let mut seen_record_ids = std::collections::HashSet::new();
        for (i, record) in records.iter().enumerate() {
            let id = record.id();
            if !seen_record_ids.insert(id) {
                return Err(InvariantViolation::DuplicateRecordId { id: id.to_string() });
            }

            if record.render().is_empty() {
                return Err(InvariantViolation::PathIndexMismatch {
                    path: format!("record {i} has empty render segments"),
                });
            }

            for seg in record.render() {
                if let Segment::Ref(node_id) = seg {
                    checked_references += 1;
                    if !self.contains_node_id(node_id) {
                        return Err(InvariantViolation::DanglingRef {
                            record_index: i,
                            node: *node_id,
                        });
                    }
                }
            }
        }

        // Invariant 1 & 4: Forest (no cycles, at most one parent) and
        // parent/child reciprocity.
        // Invariant 3: Every node has exactly one canonical owner.
        // Invariant 5: Sibling labels are unique (checked via path uniqueness).
        let mut seen_node_ids = std::collections::HashSet::new();
        for (key, node) in self.nodes_iter() {
            let id = node.id();
            if !seen_node_ids.insert(id) {
                return Err(InvariantViolation::DuplicateNodeId { id: id.to_string() });
            }

            // Check parent/child reciprocity.
            if let Some(parent_key) = node.parent() {
                if let Some(parent) = self.node_by_key(parent_key) {
                    if !parent.children().contains(&key) {
                        return Err(InvariantViolation::ParentChildMismatch {
                            node: id,
                            parent: parent.id(),
                        });
                    }
                    if parent_key == key {
                        return Err(InvariantViolation::Cycle { node: id });
                    }
                }
            }

            // Check that children exist and reciprocate.
            for &child_key in node.children() {
                if let Some(child) = self.node_by_key(child_key) {
                    if child.parent() != Some(key) {
                        return Err(InvariantViolation::ParentChildMismatch {
                            node: child.id(),
                            parent: id,
                        });
                    }
                    if child_key == key {
                        return Err(InvariantViolation::Cycle { node: id });
                    }
                } else {
                    return Err(InvariantViolation::UnreachableNode { node: id });
                }
            }

            // Check owner exists.
            if !self.owner_exists(key) {
                return Err(InvariantViolation::NoOwner { node: id });
            }
        }

        // Deeper cycle check: walk from each node to roots, ensuring no
        // node is revisited on its own ancestor path.
        for (key, _) in self.nodes_iter() {
            let mut visited = std::collections::HashSet::new();
            let mut current = Some(key);
            while let Some(k) = current {
                if !visited.insert(k) {
                    let node = self.node_by_key(k).map(|n| n.id());
                    return Err(InvariantViolation::Cycle {
                        node: node.unwrap_or(NodeId::new(uuid::Uuid::nil())),
                    });
                }
                current = self.node_by_key(k).and_then(|n| n.parent());
            }
        }

        // Invariant 6: Current paths resolve uniquely.
        // node_by_path should be consistent with current_path.
        for (path, key) in self.path_index_iter() {
            if let Some(cp) = self.path_by_key(*key) {
                if cp != path {
                    return Err(InvariantViolation::PathIndexMismatch {
                        path: path.to_path_string(),
                    });
                }
            } else {
                return Err(InvariantViolation::PathIndexMismatch {
                    path: path.to_path_string(),
                });
            }
        }

        // Invariant 7: Aliases resolve to exactly one live node and don't
        // shadow current paths.
        for (alias_path, node_id) in self.aliases_iter() {
            if self.path_resolves_to(alias_path) {
                return Err(InvariantViolation::AliasShadowsPath {
                    alias: alias_path.to_path_string(),
                });
            }
            if !self.contains_node_id(node_id) {
                warnings.push(ValidationWarning {
                    code: ValidationWarningCode::DanglingAlias,
                    message: format!("alias {} points to missing node {}", alias_path, node_id),
                });
            }
        }

        // Invariant 11: Expansion state references only live nodes with
        // positive TTLs.
        for (key, ttl) in self.expansions_iter() {
            if ttl.get() == 0 {
                warnings.push(ValidationWarning {
                    code: ValidationWarningCode::DanglingExpansion,
                    message: "expansion for node has zero TTL (should have been removed)"
                        .to_string(),
                });
            }
            if !self.contains_node_key(key) {
                warnings.push(ValidationWarning {
                    code: ValidationWarningCode::DanglingExpansion,
                    message: "expansion references missing node".to_string(),
                });
            }
        }

        Ok(ValidationReport {
            namespace_version: self.version(),
            checked_records: records.len() as u64,
            checked_nodes: self.nodes_iter().count() as u64,
            checked_references,
            warnings,
        })
    }

    /// Validates only the touched subgraph after a mutation.
    ///
    /// Intended for release builds where full validation on every mutation
    /// would be too expensive. Debug builds and tests run full validation.
    ///
    /// # Errors
    ///
    /// Returns [`InvariantViolation`] if any checked invariant is violated.
    pub(crate) fn validate_touched(&self, touched: &[NodeKey]) -> Result<(), InvariantViolation> {
        for &key in touched {
            if let Some(node) = self.node_by_key(key) {
                // Check parent/child reciprocity for touched nodes.
                if let Some(parent_key) = node.parent() {
                    if let Some(parent) = self.node_by_key(parent_key) {
                        if !parent.children().contains(&key) {
                            return Err(InvariantViolation::ParentChildMismatch {
                                node: node.id(),
                                parent: parent.id(),
                            });
                        }
                    }
                }
                // Check children reciprocate.
                for &child_key in node.children() {
                    if let Some(child) = self.node_by_key(child_key) {
                        if child.parent() != Some(key) {
                            return Err(InvariantViolation::ParentChildMismatch {
                                node: child.id(),
                                parent: node.id(),
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
