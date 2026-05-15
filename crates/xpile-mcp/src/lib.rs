//! MCP (Model Context Protocol) server.
//!
//! Exposes xpile's transpile / repair / inspect tools to IDE
//! assistants like Claude Code, mirroring the pattern in
//! `depyler-mcp` and `decy-mcp`.
//!
//! TODO: wire up actual MCP transport (likely PMCP SDK).

pub struct McpServer {
    pub bind_addr: String,
}

impl McpServer {
    pub fn new(bind_addr: impl Into<String>) -> Self {
        Self {
            bind_addr: bind_addr.into(),
        }
    }
}
