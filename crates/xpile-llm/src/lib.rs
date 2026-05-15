//! LLM invocation + content-addressed cache.
//!
//! Cache key is `sha256(source_bytes || xpile_version || model_id || skills_hash)`.
//! On cache hit, returns the exact bytes stored — byte-identical across
//! runs and machines. This is what makes the stochastic agent loop
//! reproducible at the artifact level.
//!
//! See `contracts/xpile-determinism-v1.yaml` (TODO).

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey(pub [u8; 32]);

impl CacheKey {
    pub fn compute(source: &[u8], xpile_version: &str, model_id: &str, skills_hash: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(source);
        hasher.update(b"\x00");
        hasher.update(xpile_version.as_bytes());
        hasher.update(b"\x00");
        hasher.update(model_id.as_bytes());
        hasher.update(b"\x00");
        hasher.update(skills_hash);
        let result = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&result);
        Self(bytes)
    }

    pub fn hex(&self) -> String {
        hex::encode(self.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("model invocation failed: {0}")]
    Invocation(String),
    #[error("cache error: {0}")]
    Cache(String),
}
