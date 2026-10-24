use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::NamespaceConfig;
use crate::error::NamespaceError;
use crate::event::NamespaceEvent;
use crate::id::{ContentHash, NodeId};
use crate::node::{NamespaceNode, NodeSource, OwnerLocation};
use crate::record::{Record, Segment};
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroU32;

use indexmap::IndexMap;
use slotmap::{SecondaryMap, SlotMap};

use crate::node::NodeKey;
use crate::path::NamespacePath;

/// The snapshot schema version.
pub(crate) const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// A serialized namespace snapshot with an integrity hash.
///
/// The SHA-256 hash allows the caller to verify integrity before
/// restoring.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotBytes {
    data: Vec<u8>,
    sha256_hex: String,
}

impl SnapshotBytes {
    /// Creates a snapshot from raw bytes, computing the SHA-256 hash.
    pub fn new(data: Vec<u8>) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let hash = hasher.finalize();
        let sha256_hex = hex::encode(hash);
        Self { data, sha256_hex }
    }

    /// Returns the raw snapshot bytes.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Returns the lowercase hex-encoded SHA-256 hash.
    pub fn sha256_hex(&self) -> &str {
        &self.sha256_hex
    }
}

/// Options controlling snapshot restoration.
///
/// When `min_version` or `max_version` is `Some`, the restore will reject
/// snapshots whose schema version falls outside the accepted range.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RestoreOptions {
    /// Minimum accepted schema version, or `None` to accept any.
    pub min_version: Option<u32>,
    /// Maximum accepted schema version, or `None` to accept any.
    pub max_version: Option<u32>,
}

// ---------------------------------------------------------------------------
// Snapshot envelope types (serde-compatible, using external IDs)
// ---------------------------------------------------------------------------

/// A node serialized for snapshot storage, using external IDs instead of
/// internal arena keys.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeSnapshot {
    /// The immutable node ID.
    id: NodeId,
    /// The node's label text.
    label: String,
    /// The parent node ID, or `None` for a root.
    parent_id: Option<NodeId>,
    /// IDs of immediate children.
    child_ids: Vec<NodeId>,
    /// The summary text.
    summary: String,
    /// The gloss text.
    gloss: String,
    /// The compressed source material.
    source: NodeSource,
    /// Provenance metadata.
    provenance: crate::node::Provenance,
}

/// An alias serialized for snapshot storage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AliasSnapshot {
    /// The retained alias path.
    path: NamespacePath,
    /// The live node ID the alias resolves to.
    node_id: NodeId,
}

/// An expansion serialized for snapshot storage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpansionSnapshot {
    /// The node ID with an active expansion.
    node_id: NodeId,
    /// The remaining TTL in renders.
    ttl: NonZeroU32,
}

/// The versioned snapshot envelope.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamespaceSnapshotV1 {
    /// The snapshot schema version.
    pub schema_version: u32,
    /// The library version that produced the snapshot.
    pub library_version: String,
    /// The namespace version at snapshot time.
    pub namespace_version: u64,
    /// The namespace configuration.
    pub config: NamespaceConfig,
    /// All canonical records with stable IDs.
    pub records: Vec<Record>,
    /// All nodes in canonical external-ID order.
    pub nodes: Vec<NodeSnapshot>,
    /// Retained path aliases.
    pub aliases: Vec<AliasSnapshot>,
    /// Active expansions with remaining TTLs.
    pub active_expansions: Vec<ExpansionSnapshot>,
    /// SHA-256 content hash for integrity verification.
    pub content_sha256: ContentHash,
}

impl crate::namespace::ContextNamespace {
    /// Serializes the current namespace state to a snapshot.
    ///
    /// Pending requests are excluded. The snapshot contains all canonical
    /// records, nodes, aliases, active expansions, configuration, and a
    /// content hash for integrity verification.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if serialization fails.
    pub fn snapshot(&mut self) -> Result<SnapshotBytes, NamespaceError> {
        // Serialize nodes in canonical external-ID order, not arena order.
        let mut node_snapshots: Vec<NodeSnapshot> = self
            .nodes_iter()
            .map(|(_, node)| {
                let parent_id = node
                    .parent()
                    .and_then(|pk| self.node_by_key(pk).map(|pn| pn.id()));
                let child_ids: Vec<NodeId> = node
                    .children()
                    .iter()
                    .filter_map(|&ck| self.node_by_key(ck).map(|cn| cn.id()))
                    .collect();
                NodeSnapshot {
                    id: node.id(),
                    label: node.label().as_str().to_string(),
                    parent_id,
                    child_ids,
                    summary: node.summary_text().to_string(),
                    gloss: node.gloss_text().to_string(),
                    source: node.source().clone(),
                    provenance: node.provenance().clone(),
                }
            })
            .collect();
        node_snapshots.sort_by_key(|n| n.id);

        let aliases: Vec<AliasSnapshot> = self
            .aliases_iter()
            .map(|(path, node_id)| AliasSnapshot {
                path: path.clone(),
                node_id: *node_id,
            })
            .collect();

        let active_expansions: Vec<ExpansionSnapshot> = self
            .expansions_iter()
            .filter_map(|(key, ttl)| {
                self.node_by_key(key).map(|node| ExpansionSnapshot {
                    node_id: node.id(),
                    ttl,
                })
            })
            .collect();

        // Compute content hash over the canonical state (excluding the hash
        // field itself).
        let canonical = CanonicalContent {
            namespace_version: self.version(),
            records: self.records(),
            nodes: &node_snapshots,
            aliases: &aliases,
            active_expansions: &active_expansions,
        };
        let content_sha256 = canonical.content_hash();

        let envelope = NamespaceSnapshotV1 {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            library_version: env!("CARGO_PKG_VERSION").to_string(),
            namespace_version: self.version(),
            config: self.config().clone(),
            records: self.records().to_vec(),
            nodes: node_snapshots,
            aliases,
            active_expansions,
            content_sha256,
        };

        let data = serde_json::to_vec(&envelope).map_err(|e| {
            NamespaceError::Invariant(format!("snapshot serialization failed: {e}"))
        })?;

        let snapshot = SnapshotBytes::new(data);
        self.push_event(NamespaceEvent::SnapshotSaved {
            bytes: snapshot.data().len() as u64,
        });

        Ok(snapshot)
    }

    /// Restores a namespace from a serialized snapshot.
    ///
    /// Validates the schema version, decodes the envelope with path-aware
    /// errors, checks unique IDs, rebuilds the node arena, resolves
    /// parent/child links, rebuilds paths and aliases, restores expansion
    /// state, and runs full invariant validation.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the schema version is unsupported,
    /// the data is corrupt, IDs are duplicated, or validation fails.
    pub fn restore(
        data: &[u8],
        options: RestoreOptions,
    ) -> Result<(Self, crate::receipt::RestoreReceipt), NamespaceError> {
        // Decode with path-aware error reporting.
        let envelope: NamespaceSnapshotV1 =
            serde_path_to_error::deserialize(&mut serde_json::Deserializer::from_slice(data))
                .map_err(|e| {
                    NamespaceError::Invariant(format!(
                        "snapshot decode error at {}: {}",
                        e.path(),
                        e
                    ))
                })?;

        // Validate schema version.
        if envelope.schema_version != SNAPSHOT_SCHEMA_VERSION {
            return Err(NamespaceError::SnapshotVersion {
                found: envelope.schema_version,
            });
        }

        // Check version range if specified.
        if let Some(min) = options.min_version {
            if envelope.schema_version < min {
                return Err(NamespaceError::SnapshotVersion {
                    found: envelope.schema_version,
                });
            }
        }
        if let Some(max) = options.max_version {
            if envelope.schema_version > max {
                return Err(NamespaceError::SnapshotVersion {
                    found: envelope.schema_version,
                });
            }
        }

        // Validate unique record IDs.
        let mut seen_record_ids = std::collections::HashSet::new();
        for record in &envelope.records {
            if !seen_record_ids.insert(record.id()) {
                return Err(NamespaceError::Invariant(format!(
                    "duplicate record ID: {}",
                    record.id()
                )));
            }
        }

        // Validate unique node IDs.
        let mut seen_node_ids = std::collections::HashSet::new();
        for node in &envelope.nodes {
            if !seen_node_ids.insert(node.id) {
                return Err(NamespaceError::Invariant(format!(
                    "duplicate node ID: {}",
                    node.id
                )));
            }
        }

        // Rebuild the node arena and indexes.
        let mut nodes: SlotMap<NodeKey, NamespaceNode> = SlotMap::with_key();
        let mut node_by_id: HashMap<NodeId, NodeKey> = HashMap::new();
        let mut key_by_node_id: HashMap<NodeId, NodeKey> = HashMap::new();

        // First pass: create all nodes without parent/child links.
        for ns in &envelope.nodes {
            let label = crate::path::NodeLabel::new(&ns.label, &envelope.config.limits)
                .map_err(|e| NamespaceError::Invariant(format!("invalid label: {e}")))?;
            let node = NamespaceNode::new(
                ns.id,
                label,
                ns.summary.clone(),
                ns.gloss.clone(),
                ns.source.clone(),
                ns.provenance.clone(),
            );
            let key = nodes.insert(node);
            node_by_id.insert(ns.id, key);
            key_by_node_id.insert(ns.id, key);
        }

        // Second pass: resolve parent/child links.
        for ns in &envelope.nodes {
            let key = key_by_node_id[&ns.id];
            if let Some(parent_id) = ns.parent_id {
                let parent_key = key_by_node_id.get(&parent_id).ok_or_else(|| {
                    NamespaceError::Invariant(format!(
                        "node {} references missing parent {}",
                        ns.id, parent_id
                    ))
                })?;
                nodes
                    .get_mut(key)
                    .expect("node exists")
                    .set_parent(Some(*parent_key));
            }
            for &child_id in &ns.child_ids {
                let child_key = key_by_node_id.get(&child_id).ok_or_else(|| {
                    NamespaceError::Invariant(format!(
                        "node {} references missing child {}",
                        ns.id, child_id
                    ))
                })?;
                nodes
                    .get_mut(key)
                    .expect("node exists")
                    .add_child(*child_key);
            }
        }

        // Rebuild paths, owner_by_node, and roots.
        let mut current_path: SecondaryMap<NodeKey, NamespacePath> = SecondaryMap::new();
        let mut node_by_path: HashMap<NamespacePath, NodeKey> = HashMap::new();
        let mut owner_by_node: SecondaryMap<NodeKey, OwnerLocation> = SecondaryMap::new();
        let mut roots: Vec<NodeKey> = Vec::new();

        // Determine owner locations by scanning records and node sources
        // for Ref segments.
        // First pass: scan top-level records.
        for (i, record) in envelope.records.iter().enumerate() {
            for (si, seg) in record.render().iter().enumerate() {
                if let Segment::Ref(node_id) = seg {
                    if let Some(&key) = key_by_node_id.get(node_id) {
                        // If this record has only one segment and it's a
                        // Ref, it's a carrier.
                        if record.render().len() == 1 {
                            owner_by_node
                                .insert(key, OwnerLocation::TopLevelCarrier { record_index: i });
                        } else {
                            owner_by_node.insert(
                                key,
                                OwnerLocation::TopLevelText {
                                    record_index: i,
                                    segment_index: si,
                                },
                            );
                        }
                    }
                }
            }
        }

        // Second pass: scan node sources for Refs (nested ownership).
        for ns in &envelope.nodes {
            let owner_key = key_by_node_id[&ns.id];
            if let NodeSource::RecordRange { records } = &ns.source {
                for record in records.iter() {
                    for (si, seg) in record.render().iter().enumerate() {
                        if let Segment::Ref(child_id) = seg {
                            if let Some(&child_key) = key_by_node_id.get(child_id) {
                                if record.render().len() == 1 {
                                    owner_by_node.insert(
                                        child_key,
                                        OwnerLocation::StoredCarrier { owner: owner_key },
                                    );
                                } else {
                                    owner_by_node.insert(
                                        child_key,
                                        OwnerLocation::StoredText {
                                            owner: owner_key,
                                            segment_index: si,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
            if let NodeSource::TextSlice { segments, .. } = &ns.source {
                for (si, seg) in segments.iter().enumerate() {
                    if let Segment::Ref(child_id) = seg {
                        if let Some(&child_key) = key_by_node_id.get(child_id) {
                            owner_by_node.insert(
                                child_key,
                                OwnerLocation::StoredText {
                                    owner: owner_key,
                                    segment_index: si,
                                },
                            );
                        }
                    }
                }
            }
        }

        // Build paths for root nodes (those without parents).
        for ns in &envelope.nodes {
            let key = key_by_node_id[&ns.id];
            if ns.parent_id.is_none() {
                roots.push(key);
                let label = crate::path::NodeLabel::new(&ns.label, &envelope.config.limits)
                    .map_err(|e| NamespaceError::Invariant(format!("invalid label: {e}")))?;
                let path = NamespacePath::root()
                    .child(&label, &envelope.config.limits)
                    .map_err(|e| {
                        NamespaceError::Invariant(format!("path construction failed: {e}"))
                    })?;
                current_path.insert(key, path.clone());
                node_by_path.insert(path, key);
            }
        }

        // Build paths for child nodes (BFS from roots).
        let mut queue: VecDeque<NodeKey> = roots.iter().copied().collect();
        while let Some(key) = queue.pop_front() {
            let children: Vec<NodeKey> = nodes
                .get(key)
                .map(|n| n.children().to_vec())
                .unwrap_or_default();
            let parent_path = current_path
                .get(key)
                .cloned()
                .unwrap_or_else(NamespacePath::root);
            for child_key in children {
                let label = nodes
                    .get(child_key)
                    .map(|n| n.label().clone())
                    .unwrap_or_else(|| {
                        crate::path::NodeLabel::new("child", &envelope.config.limits)
                            .expect("valid fallback label")
                    });
                let child_path =
                    parent_path
                        .child(&label, &envelope.config.limits)
                        .map_err(|e| {
                            NamespaceError::Invariant(format!("path construction failed: {e}"))
                        })?;
                current_path.insert(child_key, child_path.clone());
                node_by_path.insert(child_path, child_key);
                queue.push_back(child_key);
            }
        }

        // Rebuild aliases.
        let mut aliases: IndexMap<NamespacePath, NodeId> = IndexMap::new();
        for alias in &envelope.aliases {
            aliases.insert(alias.path.clone(), alias.node_id);
        }

        // Rebuild active expansions.
        let mut active_expansions: SecondaryMap<NodeKey, NonZeroU32> = SecondaryMap::new();
        for exp in &envelope.active_expansions {
            if let Some(&key) = key_by_node_id.get(&exp.node_id) {
                active_expansions.insert(key, exp.ttl);
            }
        }

        // Compute next_record_seq from the highest existing sequence.
        let next_record_seq = envelope
            .records
            .iter()
            .map(|r| {
                let uuid = r.id().as_uuid();
                uuid.get_timestamp()
                    .map(|ts| {
                        let (seconds, count) = ts.to_unix();
                        (seconds * 1000) + count as u64
                    })
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(0)
            + 1;

        let mut ns = Self::from_restored(
            envelope.config.clone(),
            envelope.records.clone(),
            nodes,
            node_by_id,
            roots,
            owner_by_node,
            current_path,
            node_by_path,
            aliases,
            active_expansions,
            envelope.namespace_version,
            next_record_seq,
        );

        // Validate the restored namespace.
        ns.validate().map_err(|e| {
            NamespaceError::Invariant(format!("restored namespace fails validation: {e}"))
        })?;

        let receipt = crate::receipt::RestoreReceipt {
            records: envelope.records.len() as u32,
            nodes: envelope.nodes.len() as u32,
        };

        ns.push_event(crate::event::NamespaceEvent::Restored {
            records: receipt.records,
            nodes: receipt.nodes,
        });

        Ok((ns, receipt))
    }
}

// ---------------------------------------------------------------------------
// Canonical content hashing (for snapshot integrity)
// ---------------------------------------------------------------------------

struct CanonicalContent<'a> {
    namespace_version: u64,
    records: &'a [Record],
    nodes: &'a [NodeSnapshot],
    aliases: &'a [AliasSnapshot],
    active_expansions: &'a [ExpansionSnapshot],
}

impl<'a> CanonicalContent<'a> {
    fn content_hash(&self) -> ContentHash {
        let mut hasher = Sha256::new();
        hasher.update(self.namespace_version.to_le_bytes());
        hasher.update(serde_json::to_vec(self.records).unwrap_or_default());
        hasher.update(serde_json::to_vec(self.nodes).unwrap_or_default());
        hasher.update(serde_json::to_vec(self.aliases).unwrap_or_default());
        hasher.update(serde_json::to_vec(self.active_expansions).unwrap_or_default());
        let result = hasher.finalize();
        ContentHash::of(&result)
    }
}
