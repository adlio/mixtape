// Process management tools
mod force_terminate;
mod interact_with_process;
mod kill_process;
mod list_processes;
mod list_sessions;
mod read_process_output;
mod session_manager;
mod start_process;

pub use force_terminate::ForceTerminateTool;
pub use interact_with_process::InteractWithProcessTool;
pub use kill_process::KillProcessTool;
pub use list_processes::ListProcessesTool;
pub use list_sessions::ListSessionsTool;
pub use read_process_output::ReadProcessOutputTool;
pub use start_process::StartProcessTool;

use mixtape_core::tool::{box_tool, DynTool};

/// Returns all process management tools
pub fn all_tools() -> Vec<Box<dyn DynTool>> {
    vec![
        box_tool(StartProcessTool),
        box_tool(InteractWithProcessTool),
        box_tool(ReadProcessOutputTool),
        box_tool(ListSessionsTool),
        box_tool(ListProcessesTool),
        box_tool(KillProcessTool),
        box_tool(ForceTerminateTool),
    ]
}
