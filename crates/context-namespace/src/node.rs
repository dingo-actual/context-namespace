use serde::{Deserialize, Serialize};

use crate::id::{NodeId, RecordId};
use crate::path::{NamespacePath, NodeLabel};
use crate::record::Segment;

slotmap::new_key_type! {
    /// Internal generational key into the node arena.
    ///
    /// This key is process-local and never serialized. Snapshots persist
    /// external [`NodeId`] values and rebuild the arena on restore.
    pub struct NodeKey;
}

/// Describes the compressed source material a node was created from.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NodeSource {
    /// Created by summarizing a character range within one record's render
    /// field. The segments hold the original text and any refs that were
    /// fully contained in the range.
    TextSlice {
        record_id: RecordId,
        segments: Vec<Segment>,
    },
    /// Created by summarizing a consecutive range of canonical records.
    /// Those records are preserved as the node's source material.
    RecordRange { records: Vec<crate::record::Record> },
}

/// Metadata describing how and when a node was created.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Provenance {
    /// SHA-256 hash of the original source material.
    pub source_hash: crate::id::ContentHash,
    /// SHA-256 hash of the summary result text.
    pub result_hash: crate::id::ContentHash,
    /// Namespace version at the time the node was committed.
    pub namespace_version: u64,
}

/// Serde-compatible representation of a node, replacing internal `NodeKey`
/// references with external `NodeId` values.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct NodeSerde {
    id: NodeId,
    label: NodeLabel,
    parent_id: Option<NodeId>,
    child_ids: Vec<NodeId>,
    summary: String,
    gloss: String,
    source: NodeSource,
    provenance: Provenance,
}

/// A summary node in the namespace tree.
///
/// Each node stores its immutable external ID, a label, summary text, a
/// short gloss, the compressed source material, provenance, and tree
/// connectivity via internal [`NodeKey`] references.
#[derive(Clone, Debug)]
pub struct NamespaceNode {
    id: NodeId,
    label: NodeLabel,
    parent: Option<NodeKey>,
    children: Vec<NodeKey>,
    summary: String,
    gloss: String,
    source: NodeSource,
    provenance: Provenance,
}

impl Serialize for NamespaceNode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Internal NodeKey fields are not serializable; serialize a
        // placeholder with None/empty since the real parent/child links are
        // rebuilt from the snapshot's node list during restore.
        NodeSerde {
            id: self.id,
            label: self.label.clone(),
            parent_id: None,
            child_ids: Vec::new(),
            summary: self.summary.clone(),
            gloss: self.gloss.clone(),
            source: self.source.clone(),
            provenance: self.provenance.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for NamespaceNode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = NodeSerde::deserialize(deserializer)?;
        Ok(NamespaceNode {
            id: s.id,
            label: s.label,
            parent: None,
            children: Vec::new(),
            summary: s.summary,
            gloss: s.gloss,
            source: s.source,
            provenance: s.provenance,
        })
    }
}

impl NamespaceNode {
    /// Creates a new node with the given ID, label, summary, gloss, source, and provenance.
    ///
    /// The node starts with no parent and no children; tree connectivity is
    /// set up by the namespace during commit.
    pub fn new(
        id: NodeId,
        label: NodeLabel,
        summary: String,
        gloss: String,
        source: NodeSource,
        provenance: Provenance,
    ) -> Self {
        Self {
            id,
            label,
            parent: None,
            children: Vec::new(),
            summary,
            gloss,
            source,
            provenance,
        }
    }

    /// Returns the immutable external node ID.
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Returns the node's label.
    pub fn label(&self) -> &NodeLabel {
        &self.label
    }

    /// Returns the internal key of the parent node, if any.
    pub fn parent(&self) -> Option<NodeKey> {
        self.parent
    }

    /// Returns the internal keys of the node's immediate children.
    pub fn children(&self) -> &[NodeKey] {
        &self.children
    }

    /// Returns the summary text.
    pub fn summary_text(&self) -> &str {
        &self.summary
    }

    /// Returns the gloss text.
    pub fn gloss_text(&self) -> &str {
        &self.gloss
    }

    /// Returns the compressed source material.
    pub fn source(&self) -> &NodeSource {
        &self.source
    }

    /// Returns the provenance metadata.
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub(crate) fn set_parent(&mut self, parent: Option<NodeKey>) {
        self.parent = parent;
    }

    pub(crate) fn add_child(&mut self, child: NodeKey) {
        self.children.push(child);
    }

    pub(crate) fn children_mut(&mut self) -> &mut Vec<NodeKey> {
        &mut self.children
    }
}

/// Where a node's canonical reference lives in the record sequence.
#[derive(Clone, Debug)]
pub enum OwnerLocation {
    /// The node is referenced by a segment in a top-level record's render
    /// field.
    TopLevelText {
        record_index: usize,
        segment_index: usize,
    },
    /// The node is referenced by a synthetic carrier record at the top
    /// level.
    TopLevelCarrier { record_index: usize },
    /// The node is referenced by a segment inside another node's stored
    /// source (nested).
    StoredText {
        owner: NodeKey,
        segment_index: usize,
    },
    /// The node is referenced by a carrier record inside another node's
    /// stored source (nested).
    StoredCarrier { owner: NodeKey },
}

/// Public descriptor for a node, returned by
/// [`crate::ContextNamespace::list_nodes`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeDescriptor {
    /// The immutable node ID.
    pub node_id: NodeId,
    /// The current absolute path of the node.
    pub path: NamespacePath,
    /// The node's label.
    pub label: String,
    /// The parent node ID, or `None` for a root.
    pub parent_id: Option<NodeId>,
    /// The number of immediate children.
    pub child_count: usize,
    /// The UTF-8 byte length of the summary text.
    pub summary_length: usize,
    /// The summary text.
    pub summary: String,
    /// The gloss text.
    pub gloss: String,
    /// Whether the node is currently expanded.
    pub expanded: bool,
    /// The kind of source material the node was created from.
    pub source_kind: NodeSourceKind,
}

/// Simplified description of a node's source for public inspection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum NodeSourceKind {
    /// Created from a character range within one record.
    TextSlice,
    /// Created from a consecutive range of records.
    RecordRange,
}

impl NodeSource {
    /// Returns the simplified kind of this source.
    pub fn kind(&self) -> NodeSourceKind {
        match self {
            Self::TextSlice { .. } => NodeSourceKind::TextSlice,
            Self::RecordRange { .. } => NodeSourceKind::RecordRange,
        }
    }
}
