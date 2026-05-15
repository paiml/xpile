//! Oracle trait.
//!
//! An [`Oracle`] runs the *original* source (CPython, gcc-compiled C,
//! the ruchy interpreter) on an input fixture, captures outputs, and
//! lets the agent compare them against transpiled Rust output. This
//! is the semantic gate the agent must pass to exit successfully.
//!
//! The pattern is borrowed from alchemize: extract reference values
//! *before* the agent runs, then validate against them.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum OracleError {
    #[error("capture failed: {0}")]
    Capture(String),
    #[error("comparison failed: {0}")]
    Compare(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fixture {
    pub inputs: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedOutputs {
    pub outputs: Vec<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub enum ComparisonResult {
    Match,
    Divergence {
        index: usize,
        expected: String,
        actual: String,
    },
}

pub trait Oracle: Send + Sync {
    fn language(&self) -> &'static str;

    fn capture(&self, source: &Path, fixture: &Fixture) -> Result<CapturedOutputs, OracleError>;

    fn compare(&self, expected: &CapturedOutputs, actual: &CapturedOutputs) -> ComparisonResult;
}
