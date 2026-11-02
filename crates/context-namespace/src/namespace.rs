use std::collections::{HashMap, VecDeque};
use std::num::NonZeroU32;

use indexmap::IndexMap;
use slotmap::{SecondaryMap, SlotMap};
use uuid::Uuid;

use crate::config::NamespaceConfig;
use crate::error::NamespaceError;
use crate::event::{NamespaceEvent, StructuralStats};
use crate::id::{ContentHash, NodeId, RecordId, RenderId, RequestId};
use crate::node::{NamespaceNode, NodeDescriptor, NodeKey, NodeSource, OwnerLocation};
use crate::path::{NamespacePath, NodeLabel, NodeLocator};
use crate::receipt::{AppendReceipt, CancelReceipt, ExpansionReceipt, SummaryReceipt};
use crate::record::{NewRecord, Record, Segment};
use crate::render::{MaintenanceUnit, RenderPreview};
use crate::summary::{
    ChildDescriptor, MaintenanceDecision, MaintenanceInput, PreparedSummary, RecordLocator,
    RecordSelection, SummaryInput, SummaryResult, TextSelection,
};
use crate::transaction::{
    PendingSummary, PreparedOperation, SummaryMutationPlan, TextEdit, TransactionToken,
};

/// The schema version for rendered output.
pub const RENDER_SCHEMA_VERSION: u32 = 1;

/// A render resolution entry mapping visible indices to canonical records.
#[derive(Clone, Debug)]
struct RenderResolution {
    render_id: RenderId,
    /// The namespace version at the time of rendering. A render is stale
    /// when the current version differs.
    namespace_version: u64,
    /// Maps visible index to canonical record index.
    visible_to_canonical: Vec<usize>,
}

/// The central state machine owning records, the summary tree, policy
/// enforcement, and rendering.
pub struct ContextNamespace {
    config: NamespaceConfig,
    records: Vec<Record>,
    nodes: SlotMap<NodeKey, NamespaceNode>,
    node_by_id: HashMap<NodeId, NodeKey>,
    roots: Vec<NodeKey>,
    owner_by_node: SecondaryMap<NodeKey, OwnerLocation>,
    current_path: SecondaryMap<NodeKey, NamespacePath>,
    node_by_path: HashMap<NamespacePath, NodeKey>,
    aliases: IndexMap<NamespacePath, NodeId>,
    active_expansions: SecondaryMap<NodeKey, NonZeroU32>,
    pending: HashMap<RequestId, PendingSummary>,
    version: u64,
    next_record_seq: u64,
    recent_renders: VecDeque<RenderResolution>,
    events: VecDeque<NamespaceEvent>,
    dropped_events: u32,
}

impl ContextNamespace {
    /// Creates a new empty namespace with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the configuration is invalid.
    pub fn new(config: NamespaceConfig) -> Result<Self, NamespaceError> {
        crate::policy::validate_config(&config)?;
        Ok(Self {
            config,
            records: Vec::new(),
            nodes: SlotMap::with_key(),
            node_by_id: HashMap::new(),
            roots: Vec::new(),
            owner_by_node: SecondaryMap::new(),
            current_path: SecondaryMap::new(),
            node_by_path: HashMap::new(),
            aliases: IndexMap::new(),
            active_expansions: SecondaryMap::new(),
            pending: HashMap::new(),
            version: 0,
            next_record_seq: 1,
            recent_renders: VecDeque::new(),
            events: VecDeque::new(),
            dropped_events: 0,
        })
    }

    /// Reconstructs a namespace from restored state components.
    /// Used by `snapshot::restore`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_restored(
        config: NamespaceConfig,
        records: Vec<Record>,
        nodes: SlotMap<NodeKey, NamespaceNode>,
        node_by_id: HashMap<NodeId, NodeKey>,
        roots: Vec<NodeKey>,
        owner_by_node: SecondaryMap<NodeKey, OwnerLocation>,
        current_path: SecondaryMap<NodeKey, NamespacePath>,
        node_by_path: HashMap<NamespacePath, NodeKey>,
        aliases: IndexMap<NamespacePath, NodeId>,
        active_expansions: SecondaryMap<NodeKey, NonZeroU32>,
        version: u64,
        next_record_seq: u64,
    ) -> Self {
        Self {
            config,
            records,
            nodes,
            node_by_id,
            roots,
            owner_by_node,
            current_path,
            node_by_path,
            aliases,
            active_expansions,
            pending: HashMap::new(),
            version,
            next_record_seq,
            recent_renders: VecDeque::new(),
            events: VecDeque::new(),
            dropped_events: 0,
        }
    }

    /// Constructs a namespace from an initial set of records.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the configuration is invalid or any
    /// record fails validation.
    pub fn from_records<I>(config: NamespaceConfig, records: I) -> Result<Self, NamespaceError>
    where
        I: IntoIterator<Item = NewRecord>,
    {
        let mut ns = Self::new(config)?;
        for record in records {
            ns.append(record)?;
        }
        Ok(ns)
    }

    /// Returns a reference to the namespace configuration.
    pub fn config(&self) -> &NamespaceConfig {
        &self.config
    }

    /// Returns the current namespace version.
    ///
    /// The version increments on every canonical mutation (append, commit).
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Returns structural statistics for the namespace.
    pub fn stats(&self) -> StructuralStats {
        let max_depth = self
            .roots
            .iter()
            .map(|&k| self.subtree_depth(k))
            .max()
            .unwrap_or(0);

        StructuralStats {
            records: self.records.len() as u32,
            nodes: self.nodes.len() as u32,
            active_expansions: self
                .active_expansions
                .iter()
                .filter(|(_, ttl)| ttl.get() > 0)
                .count() as u32,
            pending_requests: self.pending.len() as u32,
            alias_count: self.aliases.len() as u32,
            max_depth,
            dropped_events: self.dropped_events,
        }
    }

    /// Appends a record to the canonical sequence.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if a defensive limit is exceeded.
    pub fn append(&mut self, new_record: NewRecord) -> Result<AppendReceipt, NamespaceError> {
        let max_records = self.config.limits.max_records.get() as usize;
        if self.records.len() >= max_records {
            return Err(NamespaceError::LimitExceeded(format!(
                "record count {} >= max {}",
                self.records.len(),
                max_records
            )));
        }

        let seq = self.next_record_seq;
        self.next_record_seq += 1;

        let ts = uuid::Timestamp::from_unix(uuid::NoContext, seq, 0);
        let id = RecordId::new(Uuid::new_v7(ts));

        let visible_index = self.records.len() as u32;
        let record = Record::from_new(new_record, id, &self.config.render_field);
        self.records.push(record);

        self.version += 1;
        self.push_event(NamespaceEvent::Append {
            record_id: id,
            visible_index,
        });

        Ok(AppendReceipt {
            record_id: id,
            visible_index,
        })
    }

    /// Renders the current state as deterministic JSON.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the rendered output exceeds the
    /// configured byte limit.
    pub fn render(&mut self) -> Result<RenderedHistory, NamespaceError> {
        let render_id = RenderId::new(Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext)));
        let render_field = &self.config.render_field;
        let metadata_key = if self.config.render.metadata_key.is_empty() {
            crate::record::DEFAULT_METADATA_KEY
        } else {
            &self.config.render.metadata_key
        };

        let mut visible_to_canonical = Vec::with_capacity(self.records.len());
        let mut entries: Vec<serde_json::Value> = Vec::with_capacity(self.records.len());

        for (i, record) in self.records.iter().enumerate() {
            visible_to_canonical.push(i);

            let mut fields = record.fields_with_render(render_field);

            // Apply expansion overlay: if the record contains any Ref
            // segments, replace them with either expanded content or a
            // collapsed gloss placeholder.
            let render_segments = record.render();
            let has_ref = render_segments
                .iter()
                .any(|seg| matches!(seg, Segment::Ref(_)));

            if has_ref {
                let mut overlay_text = String::new();
                for seg in render_segments {
                    match seg {
                        Segment::Text(text) => overlay_text.push_str(text),
                        Segment::Ref(nid) => {
                            overlay_text.push_str(&self.ref_overlay(*nid));
                        }
                    }
                }
                fields.insert(render_field.to_string(), serde_json::json!(overlay_text));
            }

            if self.config.render.index_mode != crate::config::IndexMode::None {
                let ctx = match self.config.render.index_mode {
                    crate::config::IndexMode::RecordId => serde_json::json!({
                        "record_id": record.id().to_string(),
                    }),
                    crate::config::IndexMode::VisibleIndex => serde_json::json!({
                        "index": i,
                    }),
                    crate::config::IndexMode::Both => serde_json::json!({
                        "index": i,
                        "record_id": record.id().to_string(),
                        "origin": "top_level",
                    }),
                    crate::config::IndexMode::None => serde_json::Value::Null,
                };
                fields.insert(metadata_key.to_string(), ctx);
            }

            entries.push(serde_json::to_value(fields).unwrap_or(serde_json::Value::Null));
        }

        let json_value = if self.config.render.envelope {
            serde_json::json!({
                "schema_version": RENDER_SCHEMA_VERSION,
                "render_id": render_id.to_string(),
                "namespace_version": self.version,
                "records": entries,
            })
        } else {
            serde_json::Value::Array(entries)
        };

        let json_string = if self.config.render.pretty {
            serde_json::to_string_pretty(&json_value).unwrap_or_default()
        } else {
            serde_json::to_string(&json_value).unwrap_or_default()
        };

        let bytes = json_string.len() as u64;
        let max_render = self.config.limits.max_render_bytes.get();
        if bytes > max_render {
            return Err(NamespaceError::LimitExceeded(format!(
                "render bytes {bytes} exceed limit {max_render}"
            )));
        }

        let entry_count = self.records.len() as u32;

        // Store render resolution for visible-index validation.
        let max_resolutions = self.config.limits.retained_render_resolutions.get() as usize;
        self.recent_renders.push_back(RenderResolution {
            render_id,
            namespace_version: self.version,
            visible_to_canonical,
        });
        while self.recent_renders.len() > max_resolutions {
            self.recent_renders.pop_front();
        }

        // Decrement active expansion TTLs after a successful render.
        let mut expired = Vec::new();
        for (key, ttl) in self.active_expansions.iter_mut() {
            let new_val = ttl.get() - 1;
            if new_val == 0 {
                expired.push(key);
            } else {
                *ttl = NonZeroU32::new(new_val).expect("new_val > 0");
            }
        }
        for key in expired {
            self.active_expansions.remove(key);
            if let Some(node) = self.nodes.get(key) {
                self.push_event(NamespaceEvent::ExpansionExpired { node_id: node.id() });
            }
        }

        self.push_event(NamespaceEvent::Render {
            render_id,
            bytes,
            entries: entry_count,
        });

        Ok(RenderedHistory {
            render_id,
            namespace_version: self.version,
            json: json_string,
            bytes,
            entries: entry_count,
        })
    }

    /// Lists all nodes in the tree.
    pub fn list_nodes(&self) -> Vec<NodeDescriptor> {
        self.nodes
            .iter()
            .map(|(key, node)| self.describe_node(key, node))
            .collect()
    }

    /// Resolves a path to its node ID.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::UnknownPath`] if no node lives at the
    /// given path.
    pub fn resolve_path(&self, path: &NamespacePath) -> Result<NodeId, NamespaceError> {
        self.node_by_path
            .get(path)
            .and_then(|key| self.nodes.get(*key))
            .map(|n| n.id())
            .ok_or_else(|| NamespaceError::UnknownPath(path.to_path_string()))
    }

    /// Returns the current path of a node identified by its ID.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::UnknownPath`] if the node ID is not found.
    pub fn current_path(&self, node_id: NodeId) -> Result<NamespacePath, NamespaceError> {
        let key = self
            .node_by_id
            .get(&node_id)
            .copied()
            .ok_or_else(|| NamespaceError::UnknownPath(node_id.to_string()))?;
        self.current_path
            .get(key)
            .cloned()
            .ok_or_else(|| NamespaceError::UnknownPath(node_id.to_string()))
    }

    /// Expands a summary node for `ttl_renders` successful renders.
    ///
    /// During rendering, expanded nodes have their source material inlined
    /// in place of the carrier/reference segment, as a read-only overlay.
    /// Canonical state is not modified.
    ///
    /// Re-expansion sets `ttl = max(current_ttl, requested_ttl)` and never
    /// shortens an active expansion. The max expansion TTL is enforced by
    /// the policy configuration (`max_expansion_ttl`).
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the path is unknown or the TTL exceeds
    /// the configured maximum.
    pub fn expand(
        &mut self,
        path: &NamespacePath,
        ttl_renders: u32,
    ) -> Result<ExpansionReceipt, NamespaceError> {
        let max_ttl = self.config.policy.max_expansion_ttl.get();
        if ttl_renders > max_ttl {
            return Err(NamespaceError::LimitExceeded(format!(
                "expansion TTL {ttl_renders} exceeds max {max_ttl}"
            )));
        }

        let key = self
            .node_by_path
            .get(path)
            .copied()
            .ok_or_else(|| NamespaceError::UnknownPath(path.to_path_string()))?;

        let ttl_nonzero = NonZeroU32::new(ttl_renders).ok_or_else(|| {
            NamespaceError::InvalidSelection("ttl_renders must be > 0".to_string())
        })?;

        // Check max active expansions (only if this is a new activation).
        let is_new = !self.active_expansions.contains_key(key);
        if is_new {
            let max_exp = self.config.limits.max_active_expansions.get() as usize;
            if self.active_expansions.len() >= max_exp {
                return Err(NamespaceError::LimitExceeded(format!(
                    "active expansions {} >= max {max_exp}",
                    self.active_expansions.len()
                )));
            }
        }

        // Re-expansion: max(current, requested), never shortens.
        let current = self.active_expansions.get(key).copied();
        let new_ttl = match current {
            Some(c) if c >= ttl_nonzero => c,
            _ => ttl_nonzero,
        };
        self.active_expansions.insert(key, new_ttl);

        let node_id = self
            .nodes
            .get(key)
            .map(|n| n.id())
            .unwrap_or_else(|| NodeId::new(Uuid::nil()));

        let render_id = RenderId::new(Uuid::nil());
        self.push_event(NamespaceEvent::ExpansionActivated {
            node_id,
            ttl_renders: new_ttl.get(),
        });

        Ok(ExpansionReceipt {
            node_id,
            ttl_renders: new_ttl.get(),
            render_id,
        })
    }

    /// Checks whether a node is currently expanded.
    pub fn is_expanded(&self, node_id: NodeId) -> bool {
        self.node_by_id
            .get(&node_id)
            .and_then(|key| self.active_expansions.get(*key))
            .is_some()
    }

    /// Renders one structural reference for the overlay: the referenced
    /// node's stored source text inline when the node is expanded, or its
    /// collapsed marker otherwise.
    fn ref_overlay(&self, node_id: NodeId) -> String {
        let Some(key) = self.node_by_id.get(&node_id).copied() else {
            // Owners keep referenced nodes alive, so a missing node
            // indicates internal corruption; keep a visible fallback.
            return "[summary: ?]".to_string();
        };
        let Some(node) = self.nodes.get(key) else {
            return "[summary: ?]".to_string();
        };
        if self.active_expansions.contains_key(key) {
            self.stored_source_text(node)
        } else {
            self.collapsed_marker(key, node)
        }
    }

    /// Builds the collapsed marker for a node. The node's summary text
    /// leads with its current path, followed by one path-prefixed gloss
    /// per immediate child when `include_child_glosses` is enabled:
    /// `[summary(/path): summary; gloss(/child): gloss, ...]`.
    fn collapsed_marker(&self, key: NodeKey, node: &NamespaceNode) -> String {
        let path = self
            .current_path
            .get(key)
            .map(|p| p.to_path_string())
            .unwrap_or_else(|| "?".to_string());
        let mut marker = format!("[summary({path}): {}", node.summary_text());
        if self.config.render.include_child_glosses {
            let glosses: Vec<String> = node
                .children()
                .iter()
                .filter_map(|&child_key| {
                    let child = self.nodes.get(child_key)?;
                    let child_path = self
                        .current_path
                        .get(child_key)
                        .map(|p| p.to_path_string())?;
                    Some(format!("gloss({child_path}): {}", child.gloss_text()))
                })
                .collect();
            if !glosses.is_empty() {
                marker.push_str("; ");
                marker.push_str(&glosses.join(", "));
            }
        }
        marker.push(']');
        marker
    }

    /// Flattens a node's stored source into overlay text: literal text
    /// verbatim, references rendered by the same rules as the top level
    /// (stored source inline when expanded, collapsed marker otherwise),
    /// and stored records joined with newlines — mirroring how
    /// `prepare_record_summary` extracts source text for the summarizer.
    fn stored_source_text(&self, node: &NamespaceNode) -> String {
        let segment_lists: Vec<&[Segment]> = match node.source() {
            NodeSource::RecordRange { records } => {
                records.iter().map(|record| record.render()).collect()
            }
            NodeSource::TextSlice { segments, .. } => vec![segments],
        };
        segment_lists
            .iter()
            .map(|segments| {
                segments
                    .iter()
                    .map(|seg| match seg {
                        Segment::Text(text) => text.clone(),
                        Segment::Ref(nid) => self.ref_overlay(*nid),
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Clears the active expansion on a node, collapsing it immediately.
    ///
    /// Canonical state is not modified; only the expansion overlay is cleared.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::UnknownPath`] if the path does not resolve.
    pub fn collapse(&mut self, node_id: NodeId) -> Result<ExpansionReceipt, NamespaceError> {
        let key = self
            .node_by_id
            .get(&node_id)
            .copied()
            .ok_or_else(|| NamespaceError::UnknownPath(node_id.to_string()))?;
        self.clear_expansion(key);
        Ok(ExpansionReceipt {
            node_id,
            ttl_renders: 0,
            render_id: RenderId::new(uuid::Uuid::nil()),
        })
    }

    fn describe_node(&self, key: NodeKey, node: &NamespaceNode) -> NodeDescriptor {
        let path = self
            .current_path
            .get(key)
            .cloned()
            .unwrap_or_else(NamespacePath::root);
        let parent_id = node
            .parent()
            .and_then(|pk| self.nodes.get(pk))
            .map(|n| n.id());
        let expanded = self.active_expansions.get(key).is_some();
        NodeDescriptor {
            node_id: node.id(),
            path,
            label: node.label().as_str().to_string(),
            parent_id,
            child_count: node.children().len(),
            summary_length: node.summary_text().len(),
            summary: node.summary_text().to_string(),
            gloss: node.gloss_text().to_string(),
            expanded,
            source_kind: node.source().kind(),
        }
    }

    /// Computes the maximum depth of a subtree rooted at `key`.
    /// Returns 0 for a missing node, 1 for a leaf.
    fn subtree_depth(&self, key: NodeKey) -> u32 {
        let Some(node) = self.nodes.get(key) else {
            return 0;
        };
        let child_depth = node
            .children()
            .iter()
            .map(|&c| self.subtree_depth(c))
            .max()
            .unwrap_or(0);
        1 + child_depth
    }

    /// Inspects a node by path or ID, returning a detailed descriptor.
    ///
    /// Unlike [`list_nodes`](Self::list_nodes), this includes the full
    /// summary text and does not change TTL state.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::UnknownPath`] if the locator does not
    /// resolve to a live node.
    pub fn inspect_node(&self, locator: NodeLocator) -> Result<NodeDescriptor, NamespaceError> {
        let key = match locator {
            NodeLocator::Path(path) => self
                .node_by_path
                .get(&path)
                .copied()
                .ok_or_else(|| NamespaceError::UnknownPath(path.to_path_string()))?,
            NodeLocator::Id(id) => self
                .node_by_id
                .get(&id)
                .copied()
                .ok_or_else(|| NamespaceError::UnknownPath(id.to_string()))?,
        };
        let node = self
            .nodes
            .get(key)
            .ok_or_else(|| NamespaceError::UnknownPath("node not found".to_string()))?;
        Ok(self.describe_node(key, node))
    }

    /// Drains up to `limit` events from the internal event queue.
    pub fn drain_events(&mut self, limit: usize) -> Vec<NamespaceEvent> {
        let mut result = Vec::with_capacity(limit);
        for _ in 0..limit {
            match self.events.pop_front() {
                Some(event) => result.push(event),
                None => break,
            }
        }
        result
    }

    // -----------------------------------------------------------------------
    // Internal helpers for node management (used by transaction module)
    // -----------------------------------------------------------------------

    /// Allocates a new node in the arena and registers its ID and path.
    /// Does NOT set up parent/child links or ownership — the caller must
    /// do that.
    pub(crate) fn insert_node(
        &mut self,
        node: NamespaceNode,
        path: NamespacePath,
        owner: OwnerLocation,
    ) -> NodeKey {
        let id = node.id();
        let key = self.nodes.insert(node);
        self.node_by_id.insert(id, key);
        self.node_by_path.insert(path.clone(), key);
        self.current_path.insert(key, path);
        self.owner_by_node.insert(key, owner);
        key
    }

    /// Adds a root node (no parent).
    pub(crate) fn add_root(&mut self, key: NodeKey) {
        self.roots.push(key);
    }

    /// Reparents a node: removes it from its old parent's children, adds it
    /// to the new parent's children, and updates the parent field.
    pub(crate) fn reparent_node(&mut self, child: NodeKey, new_parent: NodeKey) {
        // Remove from old parent.
        let old_parent = self.nodes.get(child).and_then(|n| n.parent());
        if let Some(old_parent) = old_parent {
            if let Some(old_parent_node) = self.nodes.get_mut(old_parent) {
                old_parent_node.children_mut().retain(|&k| k != child);
            }
        }
        // Update child's parent field.
        if let Some(node) = self.nodes.get_mut(child) {
            node.set_parent(Some(new_parent));
        }
        // Add to new parent.
        if let Some(new_parent_node) = self.nodes.get_mut(new_parent) {
            new_parent_node.add_child(child);
        }
    }

    /// Updates the current path of a node and its path index entry.
    pub(crate) fn update_path(&mut self, key: NodeKey, new_path: NamespacePath) {
        if let Some(old_path) = self.current_path.get(key).cloned() {
            self.node_by_path.remove(&old_path);
        }
        self.node_by_path.insert(new_path.clone(), key);
        self.current_path.insert(key, new_path);
    }

    /// Recursively updates the path of a node and all its descendants.
    /// The node's own path is set to `new_path`, then each child's path is
    /// computed as `new_path.child(child_label)`, and so on recursively.
    pub(crate) fn update_path_recursive(&mut self, key: NodeKey, new_path: NamespacePath) {
        self.update_path(key, new_path.clone());
        let children: Vec<(NodeKey, NodeLabel)> = self
            .nodes
            .get(key)
            .map(|n| {
                n.children()
                    .iter()
                    .filter_map(|&ck| self.nodes.get(ck).map(|cn| (ck, cn.label().clone())))
                    .collect()
            })
            .unwrap_or_default();
        for (child_key, label) in children {
            if let Ok(child_path) = new_path.child(&label, &self.config.limits) {
                self.update_path_recursive(child_key, child_path);
            }
        }
    }

    /// Gets a node by its internal key.
    pub(crate) fn get_node(&self, key: NodeKey) -> Option<&NamespaceNode> {
        self.nodes.get(key)
    }

    /// Gets a node key by its external ID.
    pub(crate) fn key_by_id(&self, id: NodeId) -> Option<NodeKey> {
        self.node_by_id.get(&id).copied()
    }

    /// Gets the owner location for a node.
    pub(crate) fn owner_of(&self, key: NodeKey) -> Option<&OwnerLocation> {
        self.owner_by_node.get(key)
    }

    // -----------------------------------------------------------------------
    // Read-only accessors for validation module
    // -----------------------------------------------------------------------

    pub(crate) fn nodes_iter(&self) -> impl Iterator<Item = (NodeKey, &NamespaceNode)> {
        self.nodes.iter()
    }

    pub(crate) fn node_by_key(&self, key: NodeKey) -> Option<&NamespaceNode> {
        self.nodes.get(key)
    }

    pub(crate) fn key_by_node_id(&self, id: &crate::id::NodeId) -> Option<NodeKey> {
        self.node_by_id.get(id).copied()
    }

    pub(crate) fn contains_node_id(&self, id: &crate::id::NodeId) -> bool {
        self.node_by_id.contains_key(id)
    }

    pub(crate) fn contains_node_key(&self, key: NodeKey) -> bool {
        self.nodes.contains_key(key)
    }

    pub(crate) fn path_by_key(&self, key: NodeKey) -> Option<&NamespacePath> {
        self.current_path.get(key)
    }

    pub(crate) fn key_by_path(&self, path: &NamespacePath) -> Option<NodeKey> {
        self.node_by_path.get(path).copied()
    }

    pub(crate) fn aliases_iter(
        &self,
    ) -> impl Iterator<Item = (&NamespacePath, &crate::id::NodeId)> {
        self.aliases.iter()
    }

    pub(crate) fn path_resolves_to(&self, path: &NamespacePath) -> bool {
        self.node_by_path.contains_key(path)
    }

    pub(crate) fn path_index_iter(&self) -> impl Iterator<Item = (&NamespacePath, &NodeKey)> {
        self.node_by_path.iter()
    }

    pub(crate) fn expansions_iter(&self) -> impl Iterator<Item = (NodeKey, std::num::NonZeroU32)> {
        self.active_expansions.iter().map(|(k, &v)| (k, v))
    }

    pub(crate) fn owner_exists(&self, key: NodeKey) -> bool {
        self.owner_by_node.get(key).is_some()
    }

    /// Returns the configured limits.
    pub(crate) fn limits(&self) -> &crate::config::DefensiveLimits {
        &self.config.limits
    }

    /// Returns the render field name.
    pub(crate) fn render_field(&self) -> &str {
        &self.config.render_field
    }

    /// Returns the carrier template.
    pub(crate) fn carrier_template(&self) -> &crate::config::CarrierTemplate {
        &self.config.carrier
    }

    /// Generates a new NodeId using v7 UUID.
    pub(crate) fn new_node_id(&mut self) -> NodeId {
        let seq = self.next_record_seq;
        self.next_record_seq += 1;
        let ts = uuid::Timestamp::from_unix(uuid::NoContext, seq, 0);
        NodeId::new(Uuid::new_v7(ts))
    }

    /// Generates a new RecordId using v7 UUID.
    pub(crate) fn new_record_id(&mut self) -> RecordId {
        let seq = self.next_record_seq;
        self.next_record_seq += 1;
        let ts = uuid::Timestamp::from_unix(uuid::NoContext, seq, 0);
        RecordId::new(Uuid::new_v7(ts))
    }

    /// Returns a reference to the canonical records.
    pub(crate) fn records(&self) -> &[Record] {
        &self.records
    }

    /// Returns a mutable reference to the canonical records.
    pub(crate) fn records_mut(&mut self) -> &mut Vec<Record> {
        &mut self.records
    }

    /// Increments the namespace version (after a canonical mutation).
    pub(crate) fn bump_version(&mut self) {
        self.version += 1;
    }

    /// Pushes an event onto the event queue.
    pub(crate) fn push_event(&mut self, event: NamespaceEvent) {
        let max = self.config.limits.max_event_queue.get() as usize;
        if self.events.len() >= max {
            self.events.pop_front();
            self.dropped_events += 1;
        }
        self.events.push_back(event);
    }

    /// Returns the active expansion TTL for a node, if any.
    pub(crate) fn expansion_ttl(&self, key: NodeKey) -> Option<NonZeroU32> {
        self.active_expansions.get(key).copied()
    }

    /// Sets or updates the expansion TTL for a node.
    pub(crate) fn set_expansion_ttl(&mut self, key: NodeKey, ttl: NonZeroU32) {
        self.active_expansions.insert(key, ttl);
    }

    /// Clears the expansion TTL for a node (collapse).
    pub(crate) fn clear_expansion(&mut self, key: NodeKey) {
        self.active_expansions.remove(key);
    }

    /// Resolves a render ID to its visible-index mapping and the namespace
    /// version at the time of rendering. Returns `None` if the render ID
    /// is not found or the current version differs (stale).
    pub(crate) fn resolve_render(&self, render_id: RenderId) -> Option<(u64, &[usize])> {
        self.recent_renders
            .iter()
            .find(|r| r.render_id == render_id)
            .filter(|r| r.namespace_version == self.version)
            .map(|r| (r.namespace_version, r.visible_to_canonical.as_slice()))
    }

    /// Generates a new RequestId using v7 UUID.
    pub(crate) fn new_request_id(&mut self) -> RequestId {
        let seq = self.next_record_seq;
        self.next_record_seq += 1;
        let ts = uuid::Timestamp::from_unix(uuid::NoContext, seq, 0);
        RequestId::new(Uuid::new_v7(ts))
    }

    /// Returns the pending request count.
    pub(crate) fn pending_count(&self) -> usize {
        self.pending.len()
    }

    // -----------------------------------------------------------------------
    // Transaction: prepare / commit / cancel
    // -----------------------------------------------------------------------

    /// Effective character caps for summaries and glosses: the tighter of the
    /// policy and defensive limits.
    fn effective_summary_caps(&self) -> (u32, u32) {
        (
            self.config
                .policy
                .max_summary_chars
                .min(self.config.limits.max_summary_chars.get()),
            self.config
                .policy
                .max_gloss_chars
                .min(self.config.limits.max_gloss_chars.get()),
        )
    }

    /// Prepares a record-range summary.
    ///
    /// Validates the selection against the current canonical state,
    /// extracts source text, builds child descriptors, and creates a
    /// pending request with a hash-bound transaction token.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the render ID is stale, the range is
    /// invalid, or a defensive limit is exceeded.
    pub fn prepare_record_summary(
        &mut self,
        selection: RecordSelection,
        label: NodeLabel,
    ) -> Result<PreparedSummary, NamespaceError> {
        let render_id = selection.render_id;
        let start = selection.start_index as usize;
        let end = selection.end_index as usize;

        // Validate range.
        if start >= end {
            return Err(NamespaceError::InvalidSelection(format!(
                "start_index {start} must be < end_index {end}"
            )));
        }

        // Resolve render ID to visible-index mapping. The render is stale
        // if the namespace version has changed since it was produced.
        let (_render_version, visible_to_canonical) = self
            .resolve_render(render_id)
            .ok_or_else(|| NamespaceError::StaleRender(render_id.to_string()))?;

        if end > visible_to_canonical.len() {
            return Err(NamespaceError::InvalidSelection(format!(
                "end_index {end} exceeds visible record count {}",
                visible_to_canonical.len()
            )));
        }

        // Map visible indices to canonical indices.
        let canonical_indices: Vec<usize> = (start..end)
            .map(|vi| {
                visible_to_canonical.get(vi).copied().ok_or_else(|| {
                    NamespaceError::InvalidSelection(format!("visible index {vi} out of range"))
                })
            })
            .collect::<Result<_, _>>()?;

        self.prepare_record_range(canonical_indices, label)
    }

    /// Prepares a record-range summary over canonical record indices that the
    /// caller has already validated against the current state.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if a defensive limit is exceeded.
    fn prepare_record_range(
        &mut self,
        canonical_indices: Vec<usize>,
        label: NodeLabel,
    ) -> Result<PreparedSummary, NamespaceError> {
        // Extract source text from the selected canonical records.
        let mut source_text = String::new();
        let mut source_record_ids = Vec::new();
        let mut child_node_ids = Vec::new();

        for &ci in &canonical_indices {
            let record = &self.records[ci];
            source_record_ids.push(record.id());
            // Build text from segments, detecting carrier refs.
            for seg in record.render() {
                match seg {
                    Segment::Text(text) => source_text.push_str(text),
                    Segment::Ref(node_id) => {
                        // This is a carrier reference to a child node.
                        child_node_ids.push(*node_id);
                        // Show the nested node exactly as a collapsed
                        // render would, so the summarizer sees the same
                        // text the model would.
                        if let Some(key) = self.node_by_id.get(node_id) {
                            if let Some(node) = self.nodes.get(*key) {
                                source_text.push_str(&self.collapsed_marker(*key, node));
                            }
                        }
                    }
                }
            }
            source_text.push('\n');
        }
        // Trim trailing newline.
        if source_text.ends_with('\n') {
            source_text.pop();
        }

        // Compute source hash.
        let source_hash = ContentHash::of(source_text.as_bytes());

        // Build child descriptors.
        let children = child_node_ids
            .iter()
            .filter_map(|nid| {
                self.node_by_id.get(nid).and_then(|key| {
                    self.nodes.get(*key).map(|node| ChildDescriptor {
                        node_id: *nid,
                        label: node.label().as_str().to_string(),
                        summary: node.summary_text().to_string(),
                        gloss: node.gloss_text().to_string(),
                    })
                })
            })
            .collect::<Vec<_>>();

        // Check pending limit.
        let max_pending = self.config.limits.max_pending_requests.get() as usize;
        if self.pending.len() >= max_pending {
            return Err(NamespaceError::LimitExceeded(format!(
                "pending requests {} >= max {}",
                self.pending.len(),
                max_pending
            )));
        }

        // Check node limit.
        let max_nodes = self.config.limits.max_nodes.get() as usize;
        if self.nodes.len() >= max_nodes {
            return Err(NamespaceError::LimitExceeded(format!(
                "node count {} >= max {}",
                self.nodes.len(),
                max_nodes
            )));
        }

        let request_id = self.new_request_id();
        let version = self.version;

        let (max_summary_chars, max_gloss_chars) = self.effective_summary_caps();
        let input = SummaryInput {
            request_id,
            source: source_text,
            children,
            max_summary_chars,
            max_gloss_chars,
        };

        let token = TransactionToken::new(request_id, source_hash.clone(), version);

        // Store pending state.
        self.pending.insert(
            request_id,
            PendingSummary {
                request_id,
                created_version: version,
                source_hash,
                record_indices: canonical_indices.clone(),
                source_record_ids,
                child_node_ids,
                label,
                operation: PreparedOperation::RecordRange,
            },
        );

        // Emit event.
        self.push_event(NamespaceEvent::SummaryPrepared { request_id });

        Ok(PreparedSummary::new(request_id, input, token))
    }

    /// Prepares a text-slice summary within one record's render field.
    ///
    /// Validates the `TextSelection` (record locator + Unicode scalar-value
    /// char range), extracts the source text from the specified range, and
    /// creates a pending request. The range must not split an existing
    /// `Segment::Ref`.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the record is not found, the render ID
    /// is stale, the char range is invalid, or the range splits a Ref.
    pub fn prepare_text_summary(
        &mut self,
        selection: TextSelection,
        label: NodeLabel,
    ) -> Result<PreparedSummary, NamespaceError> {
        let start_char = selection.start_char;
        let end_char = selection.end_char;

        if start_char >= end_char {
            return Err(NamespaceError::InvalidSelection(format!(
                "start_char {start_char} must be < end_char {end_char}"
            )));
        }

        // Resolve the record.
        let record_index = match selection.record {
            RecordLocator::Id(record_id) => self
                .records
                .iter()
                .position(|r| r.id() == record_id)
                .ok_or_else(|| {
                NamespaceError::InvalidSelection(format!("record {record_id} not found"))
            })?,
            RecordLocator::Visible { render_id, index } => {
                let (_v, visible_to_canonical) = self
                    .resolve_render(render_id)
                    .ok_or_else(|| NamespaceError::StaleRender(render_id.to_string()))?;
                let vi = index as usize;
                if vi >= visible_to_canonical.len() {
                    return Err(NamespaceError::InvalidSelection(format!(
                        "visible index {vi} out of range"
                    )));
                }
                visible_to_canonical[vi]
            }
        };

        let record = &self.records[record_index];
        let record_id = record.id();

        // Walk segments to extract source text and validate Ref boundaries.
        let start = start_char as usize;
        let end = end_char as usize;
        let mut char_pos = 0usize;
        let mut source_text = String::new();
        let mut child_node_ids = Vec::new();
        let mut total_chars = 0usize;

        // First pass: compute total chars and validate range bounds.
        for seg in record.render() {
            total_chars += match seg {
                Segment::Text(text) => text.chars().count(),
                Segment::Ref(_) => 0,
            };
        }
        if end > total_chars {
            return Err(NamespaceError::InvalidSelection(format!(
                "end_char {end} exceeds render field length {total_chars}"
            )));
        }

        // Second pass: extract source text and detect Ref splits.
        for seg in record.render() {
            match seg {
                Segment::Text(text) => {
                    let seg_len = text.chars().count();
                    let seg_end = char_pos + seg_len;
                    if seg_end <= start {
                        // Before range.
                    } else if char_pos >= end {
                        // After range.
                    } else {
                        // Overlaps.
                        let chars: Vec<char> = text.chars().collect();
                        let local_start = start.saturating_sub(char_pos);
                        let local_end = end.min(seg_end) - char_pos;
                        source_text.extend(&chars[local_start..local_end]);
                    }
                    char_pos = seg_end;
                }
                Segment::Ref(node_id) => {
                    // A Ref has zero char length. Check if the range boundary
                    // falls exactly at a Ref position — that would split it
                    // only if start or end is between two text segments
                    // separated by a Ref. Since Ref has zero width, a range
                    // that includes the Ref position fully contains it.
                    // The Ref becomes a child if the range covers its
                    // position (i.e., start <= char_pos < end is always true
                    // since char_pos doesn't advance for Refs).
                    if start <= char_pos && char_pos < end {
                        child_node_ids.push(*node_id);
                    }
                    // A Ref cannot be split because it has zero char width.
                }
            }
        }

        // Compute source hash.
        let source_hash = ContentHash::of(source_text.as_bytes());

        // Build child descriptors.
        let children = child_node_ids
            .iter()
            .filter_map(|nid| {
                self.node_by_id.get(nid).and_then(|key| {
                    self.nodes.get(*key).map(|node| ChildDescriptor {
                        node_id: *nid,
                        label: node.label().as_str().to_string(),
                        summary: node.summary_text().to_string(),
                        gloss: node.gloss_text().to_string(),
                    })
                })
            })
            .collect::<Vec<_>>();

        // Check pending limit.
        let max_pending = self.config.limits.max_pending_requests.get() as usize;
        if self.pending.len() >= max_pending {
            return Err(NamespaceError::LimitExceeded(format!(
                "pending requests {} >= max {}",
                self.pending.len(),
                max_pending
            )));
        }

        // Check node limit.
        let max_nodes = self.config.limits.max_nodes.get() as usize;
        if self.nodes.len() >= max_nodes {
            return Err(NamespaceError::LimitExceeded(format!(
                "node count {} >= max {}",
                self.nodes.len(),
                max_nodes
            )));
        }

        let request_id = self.new_request_id();
        let version = self.version;

        let (max_summary_chars, max_gloss_chars) = self.effective_summary_caps();
        let input = SummaryInput {
            request_id,
            source: source_text,
            children,
            max_summary_chars,
            max_gloss_chars,
        };

        let token = TransactionToken::new(request_id, source_hash.clone(), version);

        // Store pending state.
        self.pending.insert(
            request_id,
            PendingSummary {
                request_id,
                created_version: version,
                source_hash,
                record_indices: vec![record_index],
                source_record_ids: vec![record_id],
                child_node_ids,
                label,
                operation: PreparedOperation::TextSlice {
                    record_id,
                    start_char,
                    end_char,
                },
            },
        );

        // Emit event.
        self.push_event(NamespaceEvent::SummaryPrepared { request_id });

        Ok(PreparedSummary::new(request_id, input, token))
    }
    ///
    /// Revalidates the transaction token (version, source hash, request ID),
    /// validates the summary result, builds a mutation plan off-state,
    /// checks path collisions, and applies the plan atomically.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the request is stale, the result is
    /// invalid, or a path collision occurs. After the request is
    /// authenticated, every outcome releases the pending request.
    pub fn commit_summary(
        &mut self,
        prepared: PreparedSummary,
        result: SummaryResult,
    ) -> Result<SummaryReceipt, NamespaceError> {
        let request_id = prepared.request_id;
        let token = prepared.token();

        // An unauthenticated token must not release the live request.
        let authenticated = self.pending.get(&request_id).is_some_and(|pending| {
            token.verify(request_id, &pending.source_hash, pending.created_version)
        });
        if !authenticated {
            return Err(NamespaceError::StaleSummary(request_id.to_string()));
        }

        // Every later outcome is terminal for this request: release it even
        // when the commit is rejected, so one failure cannot block a retry
        // under a pending-request limit.
        let outcome = self.finish_commit(request_id, result);
        self.pending.remove(&request_id);
        outcome
    }

    fn finish_commit(
        &mut self,
        request_id: RequestId,
        result: SummaryResult,
    ) -> Result<SummaryReceipt, NamespaceError> {
        let pending = self
            .pending
            .get(&request_id)
            .ok_or_else(|| NamespaceError::StaleSummary(request_id.to_string()))?;

        // Check version hasn't changed.
        if pending.created_version != self.version {
            return Err(NamespaceError::StaleSummary(request_id.to_string()));
        }

        // Validate summary result against the effective character caps.
        let (max_summary_chars, max_gloss_chars) = self.effective_summary_caps();
        result.validate(max_summary_chars, max_gloss_chars)?;

        // Build the mutation plan.
        let plan = self.build_mutation_plan(pending, &result)?;

        // Apply the plan.
        self.apply_mutation_plan(&plan);

        // Bump version.
        self.version += 1;

        // Build receipt.
        let carrier_record_id = plan
            .carrier_record
            .as_ref()
            .map(|r| r.id())
            .unwrap_or_else(|| {
                // Text-slice: no new carrier record; use a placeholder.
                plan.source_record_ids()
                    .first()
                    .copied()
                    .unwrap_or(RecordId::new(Uuid::nil()))
            });

        let receipt = SummaryReceipt {
            new_node_id: plan.new_node_id,
            carrier_record_id,
            replaced_record_ids: plan.source_record_ids().to_vec(),
            render_id: RenderId::new(Uuid::nil()),
        };

        self.push_event(NamespaceEvent::SummaryCommitted {
            node_id: plan.new_node_id,
            carrier_record_id,
            replaced_record_ids: receipt.replaced_record_ids.clone(),
        });

        Ok(receipt)
    }

    /// Cancels a pending summary request.
    ///
    /// Removes the pending request without mutating canonical state.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the request ID is not found.
    pub fn cancel_summary(
        &mut self,
        request_id: RequestId,
    ) -> Result<CancelReceipt, NamespaceError> {
        if self.pending.remove(&request_id).is_none() {
            return Err(NamespaceError::StaleSummary(request_id.to_string()));
        }
        self.push_event(NamespaceEvent::SummaryCancelled {
            request_id: request_id.to_string(),
        });
        Ok(CancelReceipt {
            request_id: request_id.to_string(),
        })
    }

    // -----------------------------------------------------------------------
    // Mutation plan construction (off-state)
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Render preview and maintenance planning
    // -----------------------------------------------------------------------

    /// Produces a non-mutating render preview with per-record maintenance units.
    ///
    /// Unlike [`render`](Self::render), this method does not allocate a
    /// `render_id`, register visible-index resolutions, emit a render event,
    /// or decrement expansion TTLs. The harness calls this before each model
    /// call to estimate per-record token counts, then passes those estimates
    /// to [`plan_maintenance`](Self::plan_maintenance).
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the rendered output exceeds the
    /// configured byte limit.
    pub fn preview_render(&self) -> Result<RenderPreview, NamespaceError> {
        let render_field = &self.config.render_field;
        let metadata_key = if self.config.render.metadata_key.is_empty() {
            crate::record::DEFAULT_METADATA_KEY
        } else {
            &self.config.render.metadata_key
        };

        let mut entries: Vec<serde_json::Value> = Vec::with_capacity(self.records.len());
        let mut maintenance_units = Vec::with_capacity(self.records.len());

        for (i, record) in self.records.iter().enumerate() {
            let mut fields = record.fields_with_render(render_field);
            let render_segments = record.render();
            let has_ref = render_segments
                .iter()
                .any(|seg| matches!(seg, Segment::Ref(_)));

            if has_ref {
                let mut overlay_text = String::new();
                for seg in render_segments {
                    match seg {
                        Segment::Text(text) => overlay_text.push_str(text),
                        Segment::Ref(nid) => {
                            overlay_text.push_str(&self.ref_overlay(*nid));
                        }
                    }
                }
                fields.insert(render_field.to_string(), serde_json::json!(overlay_text));
            }

            if self.config.render.index_mode != crate::config::IndexMode::None {
                let ctx = match self.config.render.index_mode {
                    crate::config::IndexMode::RecordId => serde_json::json!({
                        "record_id": record.id().to_string(),
                    }),
                    crate::config::IndexMode::VisibleIndex => serde_json::json!({
                        "index": i,
                    }),
                    crate::config::IndexMode::Both => serde_json::json!({
                        "index": i,
                        "record_id": record.id().to_string(),
                        "origin": "top_level",
                    }),
                    crate::config::IndexMode::None => serde_json::Value::Null,
                };
                fields.insert(metadata_key.to_string(), ctx);
            }

            let entry = serde_json::to_value(&fields).unwrap_or(serde_json::Value::Null);
            let fragment = serde_json::to_string(&entry).unwrap_or_default();
            let digest = ContentHash::of(fragment.as_bytes());

            maintenance_units.push(MaintenanceUnit {
                record_id: record.id(),
                render_digest: digest,
                json_fragment: fragment,
            });
            entries.push(entry);
        }

        let json_value = if self.config.render.envelope {
            serde_json::json!({
                "schema_version": RENDER_SCHEMA_VERSION,
                "namespace_version": self.version,
                "records": entries,
            })
        } else {
            serde_json::Value::Array(entries)
        };

        let json_string = if self.config.render.pretty {
            serde_json::to_string_pretty(&json_value).unwrap_or_default()
        } else {
            serde_json::to_string(&json_value).unwrap_or_default()
        };

        let bytes = json_string.len() as u64;
        let max_render = self.config.limits.max_render_bytes.get();
        if bytes > max_render {
            return Err(NamespaceError::LimitExceeded(format!(
                "render bytes {bytes} exceed limit {max_render}"
            )));
        }

        Ok(RenderPreview {
            namespace_version: self.version,
            json: json_string,
            bytes,
            entries: self.records.len() as u32,
            maintenance_units,
        })
    }

    /// Plans automatic maintenance based on harness-supplied token estimates.
    ///
    /// The harness calls [`preview_render`](Self::preview_render), estimates
    /// tokens for the full JSON and each per-record fragment, then passes
    /// those estimates here. Rust validates that the estimates match the
    /// current state (record IDs and digests), checks the trigger, and
    /// selects the oldest eligible consecutive record range large enough to
    /// target the configured `target_tokens`. If the trigger is not met or
    /// no legal range exists, returns [`MaintenanceDecision::NoAction`].
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the estimates do not match the current
    /// state (wrong record count, stale digest, duplicate estimate, or
    /// extra/missing estimates).
    pub fn plan_maintenance(
        &mut self,
        input: MaintenanceInput,
    ) -> Result<MaintenanceDecision, NamespaceError> {
        // Validate estimates against current state.
        let estimates = self.validate_maintenance_input(&input)?;

        let policy = &self.config.policy;

        // Trigger: compare whole-render estimate against trigger threshold.
        if input.estimated_next_tokens < policy.trigger_tokens {
            return Ok(MaintenanceDecision::NoAction {
                stats: self.stats(),
            });
        }

        // Determine eligible range after protecting prefix and recent records.
        let total = self.records.len();
        let prefix = policy.preserve_prefix_records as usize;
        let recent = policy.preserve_recent_records as usize;

        let eligible_start = prefix;
        let eligible_end = total.saturating_sub(recent);

        if eligible_start >= eligible_end {
            // No eligible records to summarize.
            return Ok(MaintenanceDecision::NoAction {
                stats: self.stats(),
            });
        }

        // Select the oldest eligible consecutive range whose cumulative
        // estimated tokens are large enough to target `target_tokens`.
        // Accumulate from the oldest eligible record forward until the
        // target is met or the eligible range is exhausted.
        let mut accumulated_tokens: u64 = 0;
        let target = policy.target_tokens as u64;
        let mut selected_end = eligible_start;

        for (idx, &tokens) in estimates
            .iter()
            .enumerate()
            .take(eligible_end)
            .skip(eligible_start)
        {
            accumulated_tokens += tokens as u64;
            selected_end = idx + 1;
            if accumulated_tokens >= target {
                break;
            }
        }

        // If we did not reach the target but there are eligible records,
        // summarize the entire eligible range. The spec says to select the
        // oldest eligible range large enough to target at most `target_tokens`;
        // if the full eligible range is smaller than the target, we still
        // summarize it rather than doing nothing, because the trigger fired.
        if selected_end == eligible_start {
            return Ok(MaintenanceDecision::NoAction {
                stats: self.stats(),
            });
        }

        // Prepare directly from the canonical eligible range: rendering here
        // would consume an expansion's final TTL before the caller sees it.
        let label = NodeLabel::new(format!("maintenance-{}", self.version), &self.config.limits)
            .map_err(|e| NamespaceError::InvalidSelection(e.to_string()))?;

        let canonical_indices: Vec<usize> = (eligible_start..selected_end).collect();
        let prepared = self.prepare_record_range(canonical_indices, label)?;

        Ok(MaintenanceDecision::Summarize(prepared))
    }

    /// Validates that the per-record estimates in a [`MaintenanceInput`]
    /// match the current canonical state: record count, record IDs, and
    /// render digests must all agree.
    ///
    /// Returns a vector of per-record token estimates indexed by canonical
    /// record position.
    fn validate_maintenance_input(
        &self,
        input: &MaintenanceInput,
    ) -> Result<Vec<u32>, NamespaceError> {
        if input.records.len() != self.records.len() {
            return Err(NamespaceError::PolicyViolation(format!(
                "expected {} record estimates, got {}",
                self.records.len(),
                input.records.len()
            )));
        }

        // Build a lookup from record ID to canonical index.
        let mut id_to_index: HashMap<RecordId, usize> = HashMap::new();
        for (i, record) in self.records.iter().enumerate() {
            id_to_index.insert(record.id(), i);
        }

        let mut estimates = vec![0u32; self.records.len()];
        let mut covered = vec![false; self.records.len()];

        for estimate in &input.records {
            let idx = *id_to_index.get(&estimate.record_id).ok_or_else(|| {
                NamespaceError::PolicyViolation(format!(
                    "estimate references unknown record ID {}",
                    estimate.record_id
                ))
            })?;

            if covered[idx] {
                return Err(NamespaceError::PolicyViolation(format!(
                    "duplicate estimate for record {}",
                    estimate.record_id
                )));
            }

            // Verify the digest matches the current render fragment.
            let record = &self.records[idx];
            let render_field = &self.config.render_field;
            let mut fields = record.fields_with_render(render_field);
            let render_segments = record.render();
            let has_ref = render_segments
                .iter()
                .any(|seg| matches!(seg, Segment::Ref(_)));

            if has_ref {
                let mut overlay_text = String::new();
                for seg in render_segments {
                    match seg {
                        Segment::Text(text) => overlay_text.push_str(text),
                        Segment::Ref(nid) => {
                            overlay_text.push_str(&self.ref_overlay(*nid));
                        }
                    }
                }
                fields.insert(render_field.to_string(), serde_json::json!(overlay_text));
            }

            // Reconstruct the fragment as preview_render would.
            let metadata_key = if self.config.render.metadata_key.is_empty() {
                crate::record::DEFAULT_METADATA_KEY
            } else {
                &self.config.render.metadata_key
            };

            if self.config.render.index_mode != crate::config::IndexMode::None {
                let ctx = match self.config.render.index_mode {
                    crate::config::IndexMode::RecordId => serde_json::json!({
                        "record_id": record.id().to_string(),
                    }),
                    crate::config::IndexMode::VisibleIndex => serde_json::json!({
                        "index": idx,
                    }),
                    crate::config::IndexMode::Both => serde_json::json!({
                        "index": idx,
                        "record_id": record.id().to_string(),
                        "origin": "top_level",
                    }),
                    crate::config::IndexMode::None => serde_json::Value::Null,
                };
                fields.insert(metadata_key.to_string(), ctx);
            }

            let entry = serde_json::to_value(&fields).unwrap_or(serde_json::Value::Null);
            let fragment = serde_json::to_string(&entry).unwrap_or_default();
            let expected_digest = ContentHash::of(fragment.as_bytes());

            if estimate.render_digest != expected_digest {
                return Err(NamespaceError::PolicyViolation(format!(
                    "stale digest for record {}",
                    estimate.record_id
                )));
            }

            estimates[idx] = estimate.estimated_tokens;
            covered[idx] = true;
        }

        // Every current record must be covered exactly once. Presence is
        // tracked separately from the token count so that a genuine
        // zero-token estimate is not mistaken for an omitted record.
        if let Some(missing) = covered.iter().position(|c| !c) {
            return Err(NamespaceError::PolicyViolation(format!(
                "missing estimate for record {}",
                self.records[missing].id()
            )));
        }

        Ok(estimates)
    }

    fn build_mutation_plan(
        &self,
        pending: &PendingSummary,
        result: &SummaryResult,
    ) -> Result<SummaryMutationPlan, NamespaceError> {
        let new_node_id = self.new_node_id_for_plan();
        let label = pending.label.clone();

        // Compute the new path: root path + label.
        let new_path = NamespacePath::root()
            .child(&label, &self.config.limits)
            .map_err(|e| NamespaceError::InvalidSelection(e.to_string()))?;

        // Check path collision.
        if self.node_by_path.contains_key(&new_path) {
            return Err(NamespaceError::PathCollision(new_path.to_path_string()));
        }

        // Check alias collision (alias must not shadow a current path or
        // another alias).
        if self.aliases.contains_key(&new_path) {
            return Err(NamespaceError::PathCollision(new_path.to_path_string()));
        }

        let result_hash = ContentHash::of(result.summary.as_bytes());
        let provenance = crate::node::Provenance {
            source_hash: pending.source_hash.clone(),
            result_hash,
            namespace_version: self.version,
        };

        match &pending.operation {
            PreparedOperation::RecordRange => self.build_record_range_plan(
                pending,
                result,
                new_node_id,
                new_path,
                label,
                provenance,
            ),
            PreparedOperation::TextSlice {
                record_id,
                start_char,
                end_char,
            } => self.build_text_slice_plan(
                pending,
                result,
                new_node_id,
                new_path,
                label,
                provenance,
                *record_id,
                *start_char,
                *end_char,
            ),
        }
    }

    /// Collects the distinct live nodes that a commit would reparent under the
    /// new node, rejecting the commit if that would exceed
    /// `max_children_per_node`.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::LimitExceeded`] if too many children would be
    /// reparented.
    fn collect_reparented_children(
        &self,
        child_node_ids: &[NodeId],
    ) -> Result<Vec<NodeKey>, NamespaceError> {
        let mut keys: Vec<NodeKey> = Vec::new();
        for nid in child_node_ids {
            if let Some(key) = self.node_by_id.get(nid) {
                if !keys.contains(key) {
                    keys.push(*key);
                }
            }
        }

        let max = self.config.limits.max_children_per_node.get() as usize;
        if keys.len() > max {
            return Err(NamespaceError::LimitExceeded(format!(
                "commit would create {} child nodes, max {max}",
                keys.len()
            )));
        }
        Ok(keys)
    }

    fn build_record_range_plan(
        &self,
        pending: &PendingSummary,
        result: &SummaryResult,
        new_node_id: NodeId,
        new_path: NamespacePath,
        label: NodeLabel,
        provenance: crate::node::Provenance,
    ) -> Result<SummaryMutationPlan, NamespaceError> {
        // The records being summarized become the node's source.
        let source_records: Vec<Record> = pending
            .record_indices
            .iter()
            .map(|&ci| self.records[ci].clone())
            .collect();

        let source = crate::node::NodeSource::RecordRange {
            records: source_records,
        };

        // Carrier record: synthetic record with a Ref segment.
        let carrier_id = self.new_record_id_for_plan();
        let mut carrier_fields = indexmap::IndexMap::new();
        let role = if self.config.carrier.role.is_empty() {
            "system".to_string()
        } else {
            self.config.carrier.role.clone()
        };
        carrier_fields.insert("role".to_string(), serde_json::json!(role));
        let carrier = Record::from_new(
            NewRecord::carrier(carrier_fields),
            carrier_id,
            &self.config.render_field,
        );

        // Override the carrier's render to be a single Ref segment.
        let mut carrier = carrier;
        carrier.set_render(vec![Segment::Ref(new_node_id)]);

        // Build alias additions for reparented children.
        let reparent = self.collect_reparented_children(&pending.child_node_ids)?;
        let alias_additions: Vec<(NamespacePath, NodeId)> = reparent
            .iter()
            .filter_map(|key| {
                let old_path = self.current_path.get(*key)?;
                Some((old_path.clone(), self.nodes.get(*key)?.id()))
            })
            .collect();

        // Carrier position: the first removed record's position.
        let carrier_position = pending.record_indices.first().copied();
        let remove_range = (
            pending.record_indices.first().copied().unwrap_or(0),
            pending.record_indices.last().map(|&i| i + 1).unwrap_or(0),
        );

        let owner = OwnerLocation::TopLevelCarrier {
            record_index: carrier_position.unwrap_or(0),
        };

        Ok(SummaryMutationPlan {
            new_node_id,
            new_path,
            label,
            summary: result.summary.clone(),
            gloss: result.gloss.clone(),
            source,
            provenance,
            owner,
            remove_record_range: Some(remove_range),
            carrier_record: Some(carrier),
            carrier_position,
            text_edit: None,
            reparent,
            alias_additions,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn build_text_slice_plan(
        &self,
        _pending: &PendingSummary,
        result: &SummaryResult,
        new_node_id: NodeId,
        new_path: NamespacePath,
        label: NodeLabel,
        provenance: crate::node::Provenance,
        record_id: RecordId,
        start_char: u32,
        end_char: u32,
    ) -> Result<SummaryMutationPlan, NamespaceError> {
        // Find the record in canonical state.
        let record_index = self
            .records
            .iter()
            .position(|r| r.id() == record_id)
            .ok_or_else(|| {
                NamespaceError::InvalidSelection(format!("record {record_id} not found"))
            })?;

        let record = &self.records[record_index];

        // Convert char offsets to segment-level operations.
        // Walk segments, tracking char position.
        let start = start_char as usize;
        let end = end_char as usize;

        let mut char_pos = 0usize;
        let mut seg_start = None;
        let mut seg_end = None;
        let mut leading_text = String::new();
        let mut trailing_text = String::new();
        let mut source_segments: Vec<Segment> = Vec::new();
        let mut child_node_ids = Vec::new();

        for (si, seg) in record.render().iter().enumerate() {
            let seg_len = match seg {
                Segment::Text(text) => text.chars().count(),
                Segment::Ref(_) => {
                    // Refs are atomic — they have no char length in the
                    // model-facing text. They occupy one "position" but
                    // cannot be split.
                    0
                }
            };

            let seg_end_char = char_pos + seg_len;

            let in_range = match seg {
                Segment::Ref(_) => start <= char_pos && char_pos < end,
                Segment::Text(_) => char_pos < end && seg_end_char > start,
            };

            if in_range {
                // Overlaps the range.
                if let Segment::Ref(node_id) = seg {
                    // Ref is fully contained — becomes child.
                    child_node_ids.push(*node_id);
                    source_segments.push(Segment::Ref(*node_id));
                    if seg_start.is_none() {
                        seg_start = Some(si);
                    }
                    seg_end = Some(si + 1);
                } else if let Segment::Text(text) = seg {
                    let chars: Vec<char> = text.chars().collect();
                    let local_start = start.saturating_sub(char_pos);
                    let local_end = end.min(seg_end_char) - char_pos;
                    // Text before the range within this segment.
                    if local_start > 0 {
                        leading_text = chars[..local_start].iter().collect();
                    }
                    // Text in the range becomes one stored source segment.
                    source_segments.push(Segment::Text(
                        chars[local_start..local_end].iter().collect(),
                    ));
                    // Text after the range within this segment.
                    if local_end < chars.len() {
                        trailing_text = chars[local_end..].iter().collect();
                    }
                    if seg_start.is_none() {
                        seg_start = Some(si);
                    }
                    seg_end = Some(si + 1);
                }
            }

            char_pos = seg_end_char;
        }

        let source = crate::node::NodeSource::TextSlice {
            record_id,
            segments: source_segments,
        };

        // Build new segments: leading leftovers + Ref(new_node) + trailing
        // leftovers. Whole segments outside the splice stay untouched.
        let mut new_segments = Vec::new();
        if !leading_text.is_empty() {
            new_segments.push(Segment::Text(leading_text));
        }
        new_segments.push(Segment::Ref(new_node_id));
        if !trailing_text.is_empty() {
            new_segments.push(Segment::Text(trailing_text));
        }

        let text_edit = TextEdit {
            record_index,
            start_segment: seg_start.unwrap_or(0),
            replace_count: seg_end.unwrap_or(0) - seg_start.unwrap_or(0),
            new_segments,
            prefix_text: String::new(), // already in new_segments
            suffix_text: String::new(),
        };

        // Reparent keys and alias additions for contained child nodes.
        let reparent = self.collect_reparented_children(&child_node_ids)?;
        let alias_additions: Vec<(NamespacePath, NodeId)> = reparent
            .iter()
            .filter_map(|key| {
                let old_path = self.current_path.get(*key)?;
                Some((old_path.clone(), self.nodes.get(*key)?.id()))
            })
            .collect();

        let owner = OwnerLocation::TopLevelText {
            record_index,
            segment_index: seg_start.unwrap_or(0),
        };

        Ok(SummaryMutationPlan {
            new_node_id,
            new_path,
            label,
            summary: result.summary.clone(),
            gloss: result.gloss.clone(),
            source,
            provenance,
            owner,
            remove_record_range: None,
            carrier_record: None,
            carrier_position: None,
            text_edit: Some(text_edit),
            reparent,
            alias_additions,
        })
    }

    fn apply_mutation_plan(&mut self, plan: &SummaryMutationPlan) {
        // 1. Handle record-range replacement.
        if let Some((start, end)) = plan.remove_record_range {
            // Remove the selected records.
            self.records.drain(start..end);

            // Insert carrier at the correct position.
            if let Some(carrier) = &plan.carrier_record {
                let pos = plan
                    .carrier_position
                    .unwrap_or(start)
                    .min(self.records.len());
                self.records.insert(pos, carrier.clone());
            }
        }

        // 2. Handle text-slice edit.
        if let Some(edit) = &plan.text_edit {
            if let Some(record) = self.records.get_mut(edit.record_index) {
                // Replace the segments in the range.
                let start = edit.start_segment;
                let count = edit.replace_count;
                let new_segs = edit.new_segments.clone();
                let mut new_render =
                    Vec::with_capacity(record.render().len() - count + new_segs.len());
                new_render.extend_from_slice(&record.render()[..start]);
                new_render.extend(new_segs);
                new_render.extend_from_slice(&record.render()[start + count..]);
                record.set_render(new_render);
            }
        }

        // 3. Create the new node.
        let node = NamespaceNode::new(
            plan.new_node_id,
            plan.label.clone(),
            plan.summary.clone(),
            plan.gloss.clone(),
            plan.source.clone(),
            plan.provenance.clone(),
        );

        let key = self.insert_node(node, plan.new_path.clone(), plan.owner.clone());
        self.add_root(key);

        // 4. Reparent children.
        for &child_key in &plan.reparent {
            self.reparent_node(child_key, key);
            // Recursively update the child's path and all descendant paths.
            let label = self
                .nodes
                .get(child_key)
                .map(|n| n.label().clone())
                .unwrap_or_else(|| {
                    NodeLabel::new("child", &self.config.limits).expect("valid fallback label")
                });
            if let Ok(child_path) = plan.new_path.child(&label, &self.config.limits) {
                self.update_path_recursive(child_key, child_path);
            }
        }

        // 5. Add aliases for old paths.
        for (old_path, node_id) in &plan.alias_additions {
            self.aliases.insert(old_path.clone(), *node_id);
        }
    }

    // -----------------------------------------------------------------------
    // Plan-level ID generators (don't mutate next_record_seq)
    // -----------------------------------------------------------------------

    fn new_node_id_for_plan(&self) -> NodeId {
        let seq = self.next_record_seq;
        let ts = uuid::Timestamp::from_unix(uuid::NoContext, seq, 0);
        NodeId::new(Uuid::new_v7(ts))
    }

    fn new_record_id_for_plan(&self) -> RecordId {
        let seq = self.next_record_seq + 1;
        let ts = uuid::Timestamp::from_unix(uuid::NoContext, seq, 0);
        RecordId::new(Uuid::new_v7(ts))
    }
}

// Re-export RenderedHistory for the public API.
pub use crate::render::RenderedHistory;
