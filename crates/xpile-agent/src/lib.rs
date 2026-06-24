//! Bounded agent loop for transpile repair.
//!
//! Adapted from alchemize's four-tool loop, generalized for multiple
//! source languages and cross-language boundaries.
//!
//! Tools exposed to the agent:
//!   - `read_file(path)`
//!   - `write_file_in_lang(lang, path, content)`
//!   - `cargo_build()`
//!   - `cargo_test()`
//!   - `run_hybrid_oracle()`
//!   - `apply_skill(name)`
//!
//! Exit condition: `cargo_build` && `cargo_test --oracle` pass.
//! Failure mode: budget exhaustion (iterations / tokens / wall-clock).
//!
//! PMAT-908 (Sprint Day 9) lands the first *executing* increment: a bounded,
//! fail-closed, deterministic [`repair`] loop. See [`repair::RepairLoop`].

pub mod repair;

pub use repair::{
    FfiArgCastRepair, FfiReturnCastRepair, FloatReprRepair, HybridCcRustcProbe, Probe, RepairLoop,
    RepairOutcome, RepairRule, Symptom,
};

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Budget {
    pub max_iterations: u32,
    pub max_tokens: u64,
    pub max_wall_clock: Duration,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            max_iterations: 8,
            max_tokens: 200_000,
            max_wall_clock: Duration::from_secs(300),
        }
    }
}

#[derive(Debug)]
pub struct Session {
    pub model_id: String,
    pub budget: Budget,
}

impl Session {
    pub fn new(model_id: impl Into<String>) -> Self {
        Self {
            model_id: model_id.into(),
            budget: Budget::default(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("budget exhausted: {0}")]
    BudgetExhausted(String),
    #[error("oracle failure: {0}")]
    Oracle(String),
    #[error("io failure: {0}")]
    Io(String),
}
