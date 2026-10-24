use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::config::DefensiveLimits;
use crate::error::NamespaceError;
use crate::id::RecordId;

/// The default reserved metadata key injected into rendered output.
pub const DEFAULT_METADATA_KEY: &str = "__ctx";

/// One part of a render field: literal text or a structural reference.
///
/// `Ref` variants identify summary nodes by their immutable external
/// `NodeId`. Segments are constructed only by the namespace during
/// summary commits.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Segment {
    /// Literal text from the original render field.
    Text(String),
    /// A structural reference to a namespace node, identified by the node's
    /// immutable external ID.
    Ref(crate::id::NodeId),
}

/// A validated, caller-supplied record awaiting insertion.
///
/// The render field is extracted during construction and stored separately
/// so that [`Record`] can keep it as segments without duplicating it in
/// `fields`.
#[derive(Clone, Debug)]
pub struct NewRecord {
    fields: IndexMap<String, serde_json::Value>,
    render_text: String,
}

impl NewRecord {
    /// Creates a new record from a field map, extracting the render field.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError`] if the render field is missing, not a
    /// string, or if a reserved metadata key (`__ctx`) is present in the
    /// fields.
    pub fn new(
        mut fields: IndexMap<String, serde_json::Value>,
        render_field: &str,
        limits: &DefensiveLimits,
    ) -> Result<Self, NamespaceError> {
        // Reject reserved metadata key unless it is the configured render
        // field itself (which would be unusual but not our concern here).
        if fields.contains_key(DEFAULT_METADATA_KEY) {
            return Err(NamespaceError::Config(format!(
                "reserved key {DEFAULT_METADATA_KEY} must not appear in record fields"
            )));
        }

        let render_value =
            fields
                .get(render_field)
                .ok_or_else(|| NamespaceError::MissingRenderField {
                    record_id: String::new(),
                    field: render_field.to_string(),
                })?;

        let render_text = render_value
            .as_str()
            .ok_or_else(|| NamespaceError::MissingRenderField {
                record_id: String::new(),
                field: render_field.to_string(),
            })?
            .to_string();

        let source_bytes = render_text.len();
        if source_bytes > limits.max_record_bytes.get() as usize {
            return Err(NamespaceError::LimitExceeded(format!(
                "record source bytes {source_bytes} exceed limit {}",
                limits.max_record_bytes
            )));
        }

        // Remove the render field so it cannot diverge from the segmented copy.
        fields.shift_remove(render_field);

        Ok(Self {
            fields,
            render_text,
        })
    }

    /// Returns the extracted render text.
    pub fn render_text(&self) -> &str {
        &self.render_text
    }

    /// Returns the non-render fields.
    pub fn fields(&self) -> &IndexMap<String, serde_json::Value> {
        &self.fields
    }

    /// Creates a minimal carrier record with the given fields and an empty
    /// render text. The render segments are set later via `Record::set_render`.
    pub(crate) fn carrier(fields: IndexMap<String, serde_json::Value>) -> Self {
        Self {
            fields,
            render_text: String::new(),
        }
    }
}

/// An inserted record with a stable ID and segmented render field.
///
/// The render field is stored as `Vec<Segment>` — initially a single
/// `Segment::Text`, but replaced with mixed text/ref segments after summary
/// commits. The original render field string is removed from `fields` to
/// prevent divergence.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    id: RecordId,
    fields: IndexMap<String, serde_json::Value>,
    render: Vec<Segment>,
}

impl Record {
    /// Constructs a `Record` from a `NewRecord`, assigning the given ID.
    ///
    /// The render text is stored as a single `Segment::Text`. The
    /// `_render_field` parameter is unused but retained for API symmetry
    /// with `NewRecord::new`.
    pub fn from_new(new_record: NewRecord, id: RecordId, _render_field: &str) -> Self {
        Self {
            id,
            fields: new_record.fields,
            render: vec![Segment::Text(new_record.render_text)],
        }
    }

    /// Returns the record's stable ID.
    pub fn id(&self) -> RecordId {
        self.id
    }

    /// Returns the non-render fields (the render field is excluded).
    pub fn fields(&self) -> &IndexMap<String, serde_json::Value> {
        &self.fields
    }

    /// Returns the render segments.
    pub fn render(&self) -> &[Segment] {
        &self.render
    }

    /// Reconstructs the render field as a string, concatenating all
    /// `Segment::Text` segments and skipping `Segment::Ref` segments.
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        for seg in &self.render {
            if let Segment::Text(text) = seg {
                out.push_str(text);
            }
        }
        out
    }

    #[allow(dead_code)]
    pub(crate) fn render_mut(&mut self) -> &mut Vec<Segment> {
        &mut self.render
    }

    /// Replaces the render segments entirely. Used during summary commit.
    pub(crate) fn set_render(&mut self, segments: Vec<Segment>) {
        self.render = segments;
    }

    /// Reconstructs the full fields map including the render field, for
    /// external views and snapshots.
    pub fn fields_with_render(&self, render_field: &str) -> IndexMap<String, serde_json::Value> {
        let mut out = self.fields.clone();
        out.insert(
            render_field.to_string(),
            serde_json::Value::String(self.render_text()),
        );
        out
    }
}
