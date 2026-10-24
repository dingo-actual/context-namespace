use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Immutable, opaque identity for a record.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct RecordId(Uuid);

impl RecordId {
    /// Creates a record ID from a UUID.
    pub fn new(inner: Uuid) -> Self {
        Self(inner)
    }

    /// Returns the inner UUID.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for RecordId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Immutable, opaque identity for a namespace node.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct NodeId(Uuid);

impl NodeId {
    /// Creates a node ID from a UUID.
    pub fn new(inner: Uuid) -> Self {
        Self(inner)
    }

    /// Returns the inner UUID.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Immutable identity for one rendered snapshot, used to validate visible indices.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct RenderId(Uuid);

impl RenderId {
    /// Creates a render ID from a UUID.
    pub fn new(inner: Uuid) -> Self {
        Self(inner)
    }

    /// Returns the inner UUID.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for RenderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Immutable identity for a summary request.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct RequestId(Uuid);

impl RequestId {
    /// Creates a request ID from a UUID.
    pub fn new(inner: Uuid) -> Self {
        Self(inner)
    }

    /// Returns the inner UUID.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// SHA-256 content hash used for source integrity and provenance.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ContentHash(#[serde(with = "hex_serde")] [u8; 32]);

impl ContentHash {
    /// Computes a SHA-256 hash of the given bytes.
    pub fn of(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let result = hasher.finalize();
        Self(result.into())
    }

    /// Returns the raw 32-byte hash.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns the lowercase hex encoding.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Decodes a 64-character lowercase hex string into a content hash.
    ///
    /// # Errors
    ///
    /// Returns an error message string if the input is not valid hex or
    /// not exactly 32 bytes.
    pub fn from_hex(s: &str) -> Result<Self, String> {
        let decoded = hex::decode(s).map_err(|e| format!("invalid hex: {e}"))?;
        let bytes: [u8; 32] = decoded
            .try_into()
            .map_err(|v: Vec<u8>| format!("expected 32 bytes, got {}", v.len()))?;
        Ok(Self(bytes))
    }
}

impl std::fmt::Display for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_hex())
    }
}

mod hex_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(d)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        let arr = bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("expected 32 bytes"))?;
        Ok(arr)
    }
}
