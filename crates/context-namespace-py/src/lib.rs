//! PyO3 bindings for the context-namespace Rust library.
//!
//! This crate produces the ``_native`` extension module imported by the
//! Python facade package. It wraps ContextNamespace and exposes its
//! methods through deliberately awkward private names so callers use the
//! ergonomic facade instead.

use context_namespace::{
    Capabilities, ContextNamespace, DefensiveLimits, MaintenanceDecision, MaintenanceInput,
    NamespaceConfig, NamespaceError, NamespacePath, NewRecord, NodeLabel, PathError,
    RecordSelection, RecordTokenEstimate, RenderedHistory, RestoreOptions, SummaryResult,
};
use indexmap::IndexMap;
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pythonize::depythonize;

create_exception!(context_namespace, ContextNamespaceError, PyException);
create_exception!(context_namespace, InvalidRecordError, ContextNamespaceError);
create_exception!(
    context_namespace,
    ReservedKeyCollisionError,
    InvalidRecordError
);
create_exception!(
    context_namespace,
    InvalidSelectionError,
    ContextNamespaceError
);
create_exception!(context_namespace, StaleRenderError, InvalidSelectionError);
create_exception!(context_namespace, UnknownPathError, ContextNamespaceError);
create_exception!(context_namespace, PathCollisionError, ContextNamespaceError);
create_exception!(
    context_namespace,
    PolicyViolationError,
    ContextNamespaceError
);
create_exception!(context_namespace, StaleSummaryError, ContextNamespaceError);
create_exception!(
    context_namespace,
    InvalidSummaryError,
    ContextNamespaceError
);
create_exception!(
    context_namespace,
    EmbeddingMismatchError,
    ContextNamespaceError
);
create_exception!(
    context_namespace,
    SnapshotVersionError,
    ContextNamespaceError
);
create_exception!(
    context_namespace,
    InvariantViolationError,
    ContextNamespaceError
);

/// Maps a NamespaceError to the corresponding Python exception.
fn map_error(err: NamespaceError) -> PyErr {
    match err {
        NamespaceError::MissingRenderField { field, .. } => {
            InvalidRecordError::new_err(format!("record lacks string field {field}"))
        }
        NamespaceError::StaleRender(id) => {
            StaleRenderError::new_err(format!("render {id} is stale or unknown"))
        }
        NamespaceError::StaleSummary(id) => {
            StaleSummaryError::new_err(format!("summary request {id} is stale"))
        }
        NamespaceError::UnknownPath(p) => {
            UnknownPathError::new_err(format!("unknown namespace path {p}"))
        }
        NamespaceError::PathCollision(p) => {
            PathCollisionError::new_err(format!("path collision at {p}"))
        }
        NamespaceError::InvalidSelection(msg) => {
            InvalidSelectionError::new_err(format!("selection is invalid: {msg}"))
        }
        NamespaceError::PolicyViolation(msg) => {
            PolicyViolationError::new_err(format!("policy violation: {msg}"))
        }
        NamespaceError::EmbeddingMismatch(msg) => {
            EmbeddingMismatchError::new_err(format!("embedding mismatch: {msg}"))
        }
        NamespaceError::SnapshotVersion { found } => {
            SnapshotVersionError::new_err(format!("snapshot schema {found} is unsupported"))
        }
        NamespaceError::Invariant(msg) => {
            InvariantViolationError::new_err(format!("invariant violation: {msg}"))
        }
        NamespaceError::LimitExceeded(msg) => {
            ContextNamespaceError::new_err(format!("limit exceeded: {msg}"))
        }
        NamespaceError::Config(msg) => {
            ContextNamespaceError::new_err(format!("configuration error: {msg}"))
        }
        NamespaceError::InvalidSummary(msg) => {
            InvalidSummaryError::new_err(format!("invalid summary result: {msg}"))
        }
    }
}

/// Maps a PathError to a Python UnknownPathError.
fn map_path_error(err: PathError) -> PyErr {
    UnknownPathError::new_err(err.to_string())
}

/// Recursively converts a Python object to a serde_json::Value.
///
/// Rejects non-string dict keys, non-finite floats, and unsupported
/// custom objects.
fn extract_json_value(obj: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    use pyo3::types::PyDict;
    if obj.is_none() {
        return Ok(serde_json::Value::Null);
    }
    if obj.is_instance_of::<PyDict>() {
        let dict: Bound<'_, PyDict> = obj.extract()?;
        let mut map = serde_json::Map::new();
        for (key, value) in dict.iter() {
            let key_str: String = key
                .extract()
                .map_err(|_| InvalidRecordError::new_err("record keys must be strings"))?;
            map.insert(key_str, extract_json_value(&value)?);
        }
        return Ok(serde_json::Value::Object(map));
    }
    if let Ok(b) = obj.extract::<bool>() {
        return Ok(serde_json::Value::Bool(b));
    }
    if let Ok(i) = obj.extract::<i64>() {
        return Ok(serde_json::Value::Number(serde_json::Number::from(i)));
    }
    if let Ok(f) = obj.extract::<f64>() {
        if f.is_finite() {
            if let Some(n) = serde_json::Number::from_f64(f) {
                return Ok(serde_json::Value::Number(n));
            }
        }
        return Err(InvalidRecordError::new_err(
            "non-finite float is not allowed",
        ));
    }
    if let Ok(s) = obj.extract::<String>() {
        return Ok(serde_json::Value::String(s));
    }
    if let Ok(items) = obj.extract::<Vec<Bound<'_, PyAny>>>() {
        let vals: PyResult<Vec<serde_json::Value>> =
            items.iter().map(|item| extract_json_value(item)).collect();
        return Ok(serde_json::Value::Array(vals?));
    }
    Err(InvalidRecordError::new_err(
        "unsupported Python value type in record",
    ))
}

/// Extracts a NewRecord from a Python mapping.
///
/// The mapping must contain the configured render field as a string.
fn extract_record(
    obj: &Bound<'_, PyAny>,
    render_field: &str,
    limits: &DefensiveLimits,
) -> PyResult<NewRecord> {
    let value = extract_json_value(obj)?;
    let fields: IndexMap<String, serde_json::Value> = match value {
        serde_json::Value::Object(map) => map.into_iter().collect(),
        _ => {
            return Err(InvalidRecordError::new_err("record must be a mapping/dict"));
        }
    };
    NewRecord::new(fields, render_field, limits).map_err(map_error)
}

/// Deserializes a NamespaceConfig from a Python dataclass or dict.
fn extract_config(obj: &Bound<'_, PyAny>) -> PyResult<NamespaceConfig> {
    depythonize::<NamespaceConfig>(obj)
        .map_err(|e| ContextNamespaceError::new_err(format!("invalid config: {e}")))
}

/// PyO3 wrapper around ContextNamespace.
#[pyclass(name = "_NativeNamespace", module = "context_namespace._native")]
struct PyNamespace {
    inner: ContextNamespace,
}

#[pyclass(
    name = "_PyRenderedHistory",
    module = "context_namespace._native",
    skip_from_py_object
)]
#[derive(Clone)]
struct PyRenderedHistory {
    #[pyo3(get)]
    render_id: String,
    #[pyo3(get)]
    namespace_version: u64,
    #[pyo3(get)]
    json: String,
    #[pyo3(get)]
    bytes: u64,
    #[pyo3(get)]
    entries: u32,
}

impl From<RenderedHistory> for PyRenderedHistory {
    fn from(h: RenderedHistory) -> Self {
        Self {
            render_id: h.render_id.to_string(),
            namespace_version: h.namespace_version,
            json: h.json,
            bytes: h.bytes,
            entries: h.entries,
        }
    }
}

#[pyclass(
    name = "_PyAppendReceipt",
    module = "context_namespace._native",
    skip_from_py_object
)]
#[derive(Clone)]
struct PyAppendReceipt {
    #[pyo3(get)]
    record_id: String,
    #[pyo3(get)]
    visible_index: u32,
}

#[pyclass(
    name = "_PySummaryReceipt",
    module = "context_namespace._native",
    skip_from_py_object
)]
#[derive(Clone)]
struct PySummaryReceipt {
    #[pyo3(get)]
    node_id: String,
    #[pyo3(get)]
    carrier_record_id: String,
    #[pyo3(get)]
    replaced_record_ids: Vec<String>,
    #[pyo3(get)]
    render_id: String,
}

#[pyclass(
    name = "_PyExpansionReceipt",
    module = "context_namespace._native",
    skip_from_py_object
)]
#[derive(Clone)]
struct PyExpansionReceipt {
    #[pyo3(get)]
    node_id: String,
    #[pyo3(get)]
    ttl_renders: u32,
    #[pyo3(get)]
    render_id: String,
}

#[pyclass(
    name = "_PyStructuralStats",
    module = "context_namespace._native",
    skip_from_py_object
)]
#[derive(Clone)]
struct PyStructuralStats {
    #[pyo3(get)]
    records: u32,
    #[pyo3(get)]
    nodes: u32,
    #[pyo3(get)]
    active_expansions: u32,
    #[pyo3(get)]
    pending_requests: u32,
    #[pyo3(get)]
    alias_count: u32,
    #[pyo3(get)]
    max_depth: u32,
    #[pyo3(get)]
    dropped_events: u32,
}

impl From<context_namespace::StructuralStats> for PyStructuralStats {
    fn from(s: context_namespace::StructuralStats) -> Self {
        Self {
            records: s.records,
            nodes: s.nodes,
            active_expansions: s.active_expansions,
            pending_requests: s.pending_requests,
            alias_count: s.alias_count,
            max_depth: s.max_depth,
            dropped_events: s.dropped_events,
        }
    }
}

#[pyclass(
    name = "_PyPreparedSummary",
    module = "context_namespace._native",
    skip_from_py_object
)]
struct PyPreparedSummary {
    #[pyo3(get)]
    request_id: String,
    #[pyo3(get)]
    source: String,
    #[pyo3(get)]
    children: Vec<PyChildDescriptor>,
    #[pyo3(get)]
    max_summary_chars: u32,
    #[pyo3(get)]
    max_gloss_chars: u32,
    inner: Option<context_namespace::PreparedSummary>,
}

#[pyclass(
    name = "_PyChildDescriptor",
    module = "context_namespace._native",
    skip_from_py_object
)]
#[derive(Clone)]
struct PyChildDescriptor {
    #[pyo3(get)]
    node_id: String,
    #[pyo3(get)]
    label: String,
    #[pyo3(get)]
    summary: String,
    #[pyo3(get)]
    gloss: String,
}

#[pyclass(
    name = "_PyRenderPreview",
    module = "context_namespace._native",
    skip_from_py_object
)]
#[derive(Clone)]
struct PyRenderPreview {
    #[pyo3(get)]
    namespace_version: u64,
    #[pyo3(get)]
    json: String,
    #[pyo3(get)]
    bytes: u64,
    #[pyo3(get)]
    entries: u32,
    #[pyo3(get)]
    maintenance_units: Vec<PyMaintenanceUnit>,
}

#[pyclass(
    name = "_PyMaintenanceUnit",
    module = "context_namespace._native",
    skip_from_py_object
)]
#[derive(Clone)]
struct PyMaintenanceUnit {
    #[pyo3(get)]
    record_id: String,
    #[pyo3(get)]
    render_digest: String,
    #[pyo3(get)]
    json_fragment: String,
}

#[pyclass(
    name = "_PyNodeDescriptor",
    module = "context_namespace._native",
    skip_from_py_object
)]
#[derive(Clone)]
struct PyNodeDescriptor {
    #[pyo3(get)]
    node_id: String,
    #[pyo3(get)]
    path: String,
    #[pyo3(get)]
    label: String,
    #[pyo3(get)]
    parent_id: Option<String>,
    #[pyo3(get)]
    child_count: usize,
    #[pyo3(get)]
    summary_length: usize,
    #[pyo3(get)]
    summary: String,
    #[pyo3(get)]
    gloss: String,
    #[pyo3(get)]
    expanded: bool,
    #[pyo3(get)]
    source_kind: String,
}

#[pyclass(
    name = "_PyCapabilities",
    module = "context_namespace._native",
    skip_from_py_object
)]
#[derive(Clone)]
struct PyCapabilities {
    #[pyo3(get)]
    library_version: String,
    #[pyo3(get)]
    rust_api_version: String,
    #[pyo3(get)]
    snapshot_schema: u32,
    #[pyo3(get)]
    render_schema: u32,
    #[pyo3(get)]
    policy_schema: u32,
    #[pyo3(get)]
    features: Vec<String>,
}

impl From<Capabilities> for PyCapabilities {
    fn from(c: Capabilities) -> Self {
        Self {
            library_version: c.library_version,
            rust_api_version: c.rust_api_version,
            snapshot_schema: c.snapshot_schema,
            render_schema: c.render_schema,
            policy_schema: c.policy_schema,
            features: c.features.into_iter().collect(),
        }
    }
}

/// Builds a PyPreparedSummary from a PreparedSummary, extracting public fields.
fn build_prepared_summary(prepared: context_namespace::PreparedSummary) -> PyPreparedSummary {
    let max_summary = prepared.input.max_summary_chars;
    let max_gloss = prepared.input.max_gloss_chars;
    let source = prepared.input.source.clone();
    let children = prepared
        .input
        .children
        .iter()
        .map(|child| PyChildDescriptor {
            node_id: child.node_id.to_string(),
            label: child.label.clone(),
            summary: child.summary.clone(),
            gloss: child.gloss.clone(),
        })
        .collect();
    let request_id = prepared.request_id.to_string();
    PyPreparedSummary {
        request_id,
        source,
        children,
        max_summary_chars: max_summary,
        max_gloss_chars: max_gloss,
        inner: Some(prepared),
    }
}

#[pymethods]
impl PyNamespace {
    #[new]
    fn new(config: &Bound<'_, PyAny>, records: &Bound<'_, PyAny>) -> PyResult<Self> {
        let cfg = extract_config(config)?;
        let record_list: Vec<Bound<'_, PyAny>> = if records.is_none() {
            Vec::new()
        } else {
            records.extract()?
        };
        let limits = cfg.limits.clone();
        let render_field = cfg.render_field.clone();
        let new_records: PyResult<Vec<NewRecord>> = record_list
            .iter()
            .map(|r| extract_record(r, &render_field, &limits))
            .collect();
        let recs = new_records?;
        let ns = if recs.is_empty() {
            ContextNamespace::new(cfg)
        } else {
            ContextNamespace::from_records(cfg, recs)
        }
        .map_err(map_error)?;
        Ok(Self { inner: ns })
    }

    fn append(&mut self, record: &Bound<'_, PyAny>) -> PyResult<PyAppendReceipt> {
        let limits = self.inner.config().limits.clone();
        let render_field = self.inner.config().render_field.clone();
        let new_record = extract_record(record, &render_field, &limits)?;
        let receipt = self.inner.append(new_record).map_err(map_error)?;
        Ok(PyAppendReceipt {
            record_id: receipt.record_id.to_string(),
            visible_index: receipt.visible_index,
        })
    }

    fn render(&mut self) -> PyResult<PyRenderedHistory> {
        let h = self.inner.render().map_err(map_error)?;
        Ok(h.into())
    }

    fn _preview_render(&self) -> PyResult<PyRenderPreview> {
        let preview = self.inner.preview_render().map_err(map_error)?;
        let units: Vec<PyMaintenanceUnit> = preview
            .maintenance_units
            .iter()
            .map(|u| PyMaintenanceUnit {
                record_id: u.record_id.to_string(),
                render_digest: u.render_digest.to_string(),
                json_fragment: u.json_fragment.clone(),
            })
            .collect();
        Ok(PyRenderPreview {
            namespace_version: preview.namespace_version,
            json: preview.json,
            bytes: preview.bytes,
            entries: preview.entries,
            maintenance_units: units,
        })
    }

    fn _prepare_records(
        &mut self,
        render_id: &str,
        start: u32,
        end: u32,
        label: &str,
    ) -> PyResult<PyPreparedSummary> {
        let limits = self.inner.config().limits.clone();
        let parsed_render_id: uuid::Uuid = render_id
            .parse()
            .map_err(|_| StaleRenderError::new_err(format!("invalid render_id: {render_id}")))?;
        let node_label = NodeLabel::new(label, &limits).map_err(map_path_error)?;
        let prepared = self
            .inner
            .prepare_record_summary(
                RecordSelection {
                    render_id: context_namespace::RenderId::new(parsed_render_id),
                    start_index: start,
                    end_index: end,
                },
                node_label,
            )
            .map_err(map_error)?;
        Ok(build_prepared_summary(prepared))
    }

    fn _prepare_text(
        &mut self,
        render_id: &str,
        index: u32,
        start_char: u32,
        end_char: u32,
        label: &str,
    ) -> PyResult<PyPreparedSummary> {
        let limits = self.inner.config().limits.clone();
        let parsed_render_id: uuid::Uuid = render_id
            .parse()
            .map_err(|_| StaleRenderError::new_err(format!("invalid render_id: {render_id}")))?;
        let node_label = NodeLabel::new(label, &limits).map_err(map_path_error)?;
        let prepared = self
            .inner
            .prepare_text_summary(
                context_namespace::TextSelection {
                    record: context_namespace::RecordLocator::Visible {
                        render_id: context_namespace::RenderId::new(parsed_render_id),
                        index,
                    },
                    start_char,
                    end_char,
                },
                node_label,
            )
            .map_err(map_error)?;
        Ok(build_prepared_summary(prepared))
    }

    fn _commit_summary(
        &mut self,
        mut prepared: PyRefMut<PyPreparedSummary>,
        summary: &Bound<'_, PyAny>,
        gloss: &Bound<'_, PyAny>,
    ) -> PyResult<PySummaryReceipt> {
        let inner = prepared
            .inner
            .take()
            .ok_or_else(|| StaleSummaryError::new_err("prepared summary already consumed"))?;
        let unpack = || -> PyResult<SummaryResult> {
            Ok(SummaryResult {
                summary: summary.extract()?,
                gloss: gloss.extract()?,
            })
        };
        let result = match unpack() {
            Ok(result) => result,
            Err(err) => {
                // Release the handle's pending request: a conversion failure
                // must not block later prepares.
                let _ = self.inner.cancel_summary(inner.request_id);
                return Err(err);
            }
        };
        let receipt = self
            .inner
            .commit_summary(inner, result)
            .map_err(map_error)?;
        Ok(PySummaryReceipt {
            node_id: receipt.new_node_id.to_string(),
            carrier_record_id: receipt.carrier_record_id.to_string(),
            replaced_record_ids: receipt
                .replaced_record_ids
                .iter()
                .map(|id| id.to_string())
                .collect(),
            render_id: receipt.render_id.to_string(),
        })
    }

    fn _cancel(&mut self, mut prepared: PyRefMut<PyPreparedSummary>) -> PyResult<()> {
        let inner = prepared
            .inner
            .take()
            .ok_or_else(|| StaleSummaryError::new_err("prepared summary already consumed"))?;
        self.inner
            .cancel_summary(inner.request_id)
            .map_err(map_error)?;
        Ok(())
    }

    fn _plan_maintenance(
        &mut self,
        input: &Bound<'_, PyAny>,
    ) -> PyResult<Option<PyPreparedSummary>> {
        let estimated_next_tokens: u32 = input.getattr("estimated_next_tokens")?.extract()?;
        let records_obj = input.getattr("records")?;
        let records_list: Vec<Bound<'_, PyAny>> = records_obj.extract()?;
        let records: PyResult<Vec<RecordTokenEstimate>> = records_list
            .iter()
            .map(|r| {
                let record_id_str: String = r.getattr("record_id")?.extract()?;
                let render_digest: String = r.getattr("render_digest")?.extract()?;
                let estimated_tokens: u32 = r.getattr("estimated_tokens")?.extract()?;
                let rid: uuid::Uuid = record_id_str.parse().map_err(|_| {
                    InvalidSelectionError::new_err(format!("invalid record_id: {record_id_str}"))
                })?;
                let digest =
                    context_namespace::ContentHash::from_hex(&render_digest).map_err(|e| {
                        InvalidSelectionError::new_err(format!("invalid render_digest: {e}"))
                    })?;
                Ok(RecordTokenEstimate {
                    record_id: context_namespace::RecordId::new(rid),
                    render_digest: digest,
                    estimated_tokens,
                })
            })
            .collect();
        let maintenance_input = MaintenanceInput {
            estimated_next_tokens,
            records: records?,
        };
        let decision = self
            .inner
            .plan_maintenance(maintenance_input)
            .map_err(map_error)?;
        match decision {
            MaintenanceDecision::NoAction { .. } => Ok(None),
            MaintenanceDecision::Summarize(prepared) => Ok(Some(build_prepared_summary(prepared))),
        }
    }

    fn expand(&mut self, path: &str, ttl: u32) -> PyResult<PyExpansionReceipt> {
        let limits = self.inner.config().limits.clone();
        let parsed = NamespacePath::parse(path, &limits).map_err(map_path_error)?;
        let receipt = self.inner.expand(&parsed, ttl).map_err(map_error)?;
        Ok(PyExpansionReceipt {
            node_id: receipt.node_id.to_string(),
            ttl_renders: receipt.ttl_renders,
            render_id: receipt.render_id.to_string(),
        })
    }

    fn collapse(&mut self, path: &str) -> PyResult<PyExpansionReceipt> {
        let limits = self.inner.config().limits.clone();
        let parsed = NamespacePath::parse(path, &limits).map_err(map_path_error)?;
        let node_id = self.inner.resolve_path(&parsed).map_err(map_error)?;
        let receipt = self.inner.collapse(node_id).map_err(map_error)?;
        Ok(PyExpansionReceipt {
            node_id: receipt.node_id.to_string(),
            ttl_renders: receipt.ttl_renders,
            render_id: receipt.render_id.to_string(),
        })
    }

    fn is_expanded(&self, path: &str) -> PyResult<bool> {
        let limits = self.inner.config().limits.clone();
        let parsed = NamespacePath::parse(path, &limits).map_err(map_path_error)?;
        let node_id = self.inner.resolve_path(&parsed).map_err(map_error)?;
        Ok(self.inner.is_expanded(node_id))
    }

    fn list_nodes(&self) -> PyResult<Vec<PyNodeDescriptor>> {
        let nodes = self.inner.list_nodes();
        Ok(nodes
            .into_iter()
            .map(|n| PyNodeDescriptor {
                node_id: n.node_id.to_string(),
                path: n.path.to_string(),
                label: n.label,
                parent_id: n.parent_id.map(|id| id.to_string()),
                child_count: n.child_count,
                summary_length: n.summary_length,
                summary: n.summary,
                gloss: n.gloss,
                expanded: n.expanded,
                source_kind: format!("{:?}", n.source_kind),
            })
            .collect())
    }

    fn stats(&self) -> PyResult<PyStructuralStats> {
        Ok(self.inner.stats().into())
    }

    fn capabilities(&self) -> PyResult<PyCapabilities> {
        Ok(Capabilities::detect().into())
    }

    fn validate(&self) -> PyResult<()> {
        self.inner
            .validate()
            .map_err(|e| map_error(NamespaceError::Invariant(e.to_string())))?;
        Ok(())
    }

    fn snapshot(&mut self, py: Python<'_>) -> PyResult<Py<PyBytes>> {
        let snap = self.inner.snapshot().map_err(map_error)?;
        Ok(PyBytes::new(py, snap.data()).unbind())
    }

    #[staticmethod]
    fn restore(data: &Bound<'_, PyBytes>) -> PyResult<Self> {
        let bytes = data.as_bytes();
        let (ns, _receipt) =
            ContextNamespace::restore(bytes, RestoreOptions::default()).map_err(map_error)?;
        Ok(Self { inner: ns })
    }

    fn version(&self) -> PyResult<u64> {
        Ok(self.inner.version())
    }

    fn drain_events(&mut self, limit: usize) -> PyResult<Vec<String>> {
        let events = self.inner.drain_events(limit);
        Ok(events.into_iter().map(|e| format!("{e:?}")).collect())
    }
}

#[pymodule]
mod _native {
    #[pymodule_export]
    use super::ContextNamespaceError;
    #[pymodule_export]
    use super::EmbeddingMismatchError;
    #[pymodule_export]
    use super::InvalidRecordError;
    #[pymodule_export]
    use super::InvalidSelectionError;
    #[pymodule_export]
    use super::InvalidSummaryError;
    #[pymodule_export]
    use super::InvariantViolationError;
    #[pymodule_export]
    use super::PathCollisionError;
    #[pymodule_export]
    use super::PolicyViolationError;
    #[pymodule_export]
    use super::PyAppendReceipt;
    #[pymodule_export]
    use super::PyCapabilities;
    #[pymodule_export]
    use super::PyChildDescriptor;
    #[pymodule_export]
    use super::PyExpansionReceipt;
    #[pymodule_export]
    use super::PyMaintenanceUnit;
    #[pymodule_export]
    use super::PyNamespace;
    #[pymodule_export]
    use super::PyNodeDescriptor;
    #[pymodule_export]
    use super::PyPreparedSummary;
    #[pymodule_export]
    use super::PyRenderPreview;
    #[pymodule_export]
    use super::PyRenderedHistory;
    #[pymodule_export]
    use super::PyStructuralStats;
    #[pymodule_export]
    use super::PySummaryReceipt;
    #[pymodule_export]
    use super::ReservedKeyCollisionError;
    #[pymodule_export]
    use super::SnapshotVersionError;
    #[pymodule_export]
    use super::StaleRenderError;
    #[pymodule_export]
    use super::StaleSummaryError;
    #[pymodule_export]
    use super::UnknownPathError;
}
