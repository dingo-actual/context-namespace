use serde::{Deserialize, Serialize};

use crate::config::DefensiveLimits;
use crate::id::NodeId;

/// Error returned by path and label parsing.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    /// A path segment is empty.
    #[error("path segment is empty")]
    EmptySegment,

    /// The path exceeds the configured maximum depth.
    #[error("path exceeds maximum depth of {0}")]
    DepthExceeded(u32),

    /// The label exceeds the configured maximum length.
    #[error("label exceeds maximum length of {0} characters")]
    LabelTooLong(u32),

    /// The label contains control characters.
    #[error("label contains control characters")]
    ControlChar,

    /// The path does not start with `/`.
    #[error("path must start with '/'")]
    NotAbsolute,

    /// The path is empty.
    #[error("path is empty")]
    Empty,
}

/// A validated, normalized label for a namespace node segment.
///
/// Labels are NFC-normalized, nonempty, bounded, and free of control
/// characters. They are constructed through [`NodeLabel::new`] which enforces
/// these invariants against the supplied [`DefensiveLimits`].
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct NodeLabel(String);

impl NodeLabel {
    /// Creates a label from a raw string, validating it against the supplied
    /// limits.
    ///
    /// # Errors
    ///
    /// Returns [`PathError`] if the label is empty, too long, or contains
    /// control characters.
    pub fn new(raw: impl Into<String>, limits: &DefensiveLimits) -> Result<Self, PathError> {
        let raw: String = raw.into();
        if raw.is_empty() {
            return Err(PathError::EmptySegment);
        }
        let normalized =
            unicode_normalization::UnicodeNormalization::nfc(raw.as_str()).collect::<String>();
        let max = limits.max_label_length.get();
        if normalized.chars().count() > max as usize {
            return Err(PathError::LabelTooLong(max));
        }
        if normalized.chars().any(char::is_control) {
            return Err(PathError::ControlChar);
        }
        Ok(Self(normalized))
    }

    /// Returns the label text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A typed segment array representing an absolute namespace path.
///
/// Paths are absolute (start with `/`), use JSON Pointer escaping for
/// segments (`~` becomes `~0`, `/` becomes `~1`), and are built through
/// [`NamespacePath::parse`] or [`NamespacePath::child`].
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct NamespacePath(Vec<String>);

impl NamespacePath {
    /// The root path, containing no segments.
    pub fn root() -> Self {
        Self(Vec::new())
    }

    /// Parses an absolute path string into a typed path.
    ///
    /// # Errors
    ///
    /// Returns [`PathError`] if the input is not absolute, contains empty
    /// segments, or exceeds the configured depth limit.
    pub fn parse(input: &str, limits: &DefensiveLimits) -> Result<Self, PathError> {
        if input.is_empty() {
            return Err(PathError::Empty);
        }
        if !input.starts_with('/') {
            return Err(PathError::NotAbsolute);
        }
        let mut segments = Vec::new();
        for seg in input.split('/') {
            if seg.is_empty() {
                continue;
            }
            // Unescape JSON Pointer encoding: ~1 -> /, ~0 -> ~
            let unescaped = seg.replace("~1", "/").replace("~0", "~");
            segments.push(unescaped);
        }
        let max_depth = limits.max_tree_depth.get() as usize;
        if segments.len() > max_depth {
            return Err(PathError::DepthExceeded(limits.max_tree_depth.get()));
        }
        Ok(Self(segments))
    }

    /// Creates a child path by appending a label.
    ///
    /// # Errors
    ///
    /// Returns [`PathError`] if the resulting path exceeds the depth limit.
    pub fn child(&self, label: &NodeLabel, limits: &DefensiveLimits) -> Result<Self, PathError> {
        let max_depth = limits.max_tree_depth.get() as usize;
        if self.0.len() + 1 > max_depth {
            return Err(PathError::DepthExceeded(limits.max_tree_depth.get()));
        }
        let mut segments = self.0.clone();
        segments.push(label.as_str().to_string());
        Ok(Self(segments))
    }

    /// Returns an iterator over the path segments.
    pub fn segments(&self) -> impl ExactSizeIterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }

    /// Renders the path as an absolute string with JSON Pointer escaping.
    pub fn to_path_string(&self) -> String {
        if self.0.is_empty() {
            return "/".to_string();
        }
        let mut out = String::new();
        for seg in &self.0 {
            out.push('/');
            out.push_str(&seg.replace("~", "~0").replace("/", "~1"));
        }
        out
    }

    /// Returns the number of segments.
    pub fn depth(&self) -> usize {
        self.0.len()
    }
}

impl std::fmt::Display for NamespacePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_path_string())
    }
}

/// A locator for a namespace node, accepting either a path or an ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NodeLocator {
    /// Locate by current absolute path.
    Path(NamespacePath),
    /// Locate by immutable node ID.
    Id(NodeId),
}

impl NodeLocator {
    /// Creates a path-based locator.
    pub fn from_path(path: NamespacePath) -> Self {
        Self::Path(path)
    }

    /// Creates an ID-based locator.
    pub fn from_id(id: NodeId) -> Self {
        Self::Id(id)
    }
}
