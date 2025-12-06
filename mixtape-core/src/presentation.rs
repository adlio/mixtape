/// Display context for tool presentation
///
/// Indicates the target display format for tool inputs and outputs.
/// This allows the agent to select the appropriate presenter for the context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    /// Command-line interface presentation (ANSI-formatted text)
    Cli,
    // Future: Web, Tui, etc.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_cli_variant() {
        let display = Display::Cli;
        assert_eq!(display, Display::Cli);
    }

    #[test]
    fn test_display_clone() {
        let display = Display::Cli;
        #[allow(clippy::clone_on_copy)] // Intentionally testing Clone trait
        let cloned = display.clone();
        assert_eq!(display, cloned);
    }

    #[test]
    fn test_display_copy() {
        let display = Display::Cli;
        let copied = display; // Copy, not move
        assert_eq!(display, copied);
    }

    #[test]
    fn test_display_debug() {
        let display = Display::Cli;
        let debug_str = format!("{:?}", display);
        assert_eq!(debug_str, "Cli");
    }
}
