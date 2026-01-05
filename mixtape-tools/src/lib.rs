pub mod aws;
pub mod edit;
pub mod fetch;
pub mod filesystem;
pub mod process;
pub mod search;
#[cfg(feature = "sqlite")]
pub mod sqlite;
pub mod utils;

// Re-export validate_path at crate root for convenience
pub use filesystem::validate_path;

// Re-export tool grouping functions at crate root for convenience
pub use filesystem::{
    all_tools as all_filesystem_tools, mutative_tools as mutative_filesystem_tools,
    read_only_tools as read_only_filesystem_tools,
};
pub use process::all_tools as all_process_tools;

/// Re-export commonly used types for convenience
pub mod prelude {
    pub use mixtape_core::{Tool, ToolError, ToolResult};
    pub use schemars::JsonSchema;
    pub use serde::{Deserialize, Serialize};
}
