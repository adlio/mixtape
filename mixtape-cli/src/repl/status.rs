//! Persistent status line display

use crossterm::{
    cursor,
    terminal::{self, ClearType},
    ExecutableCommand, QueueableCommand,
};
use mixtape_core::Agent;
use std::io::{stdout, Write};

/// ANSI color codes for status display
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusColors {
    /// Foreground color ANSI code
    pub fg: &'static str,
    /// Background color ANSI code
    pub bg: &'static str,
}

impl StatusColors {
    /// Red text on dark red background (critical: >90% usage)
    pub const CRITICAL: Self = Self {
        fg: "\x1b[31m",
        bg: "\x1b[48;5;52m",
    };

    /// Yellow text on dark yellow background (warning: >75% usage)
    pub const WARNING: Self = Self {
        fg: "\x1b[33m",
        bg: "\x1b[48;5;58m",
    };

    /// White text on gray background (normal)
    pub const NORMAL: Self = Self {
        fg: "\x1b[37m",
        bg: "\x1b[48;5;236m",
    };
}

/// Select appropriate colors based on context usage percentage
///
/// - usage >= 0.9 → Critical (red)
/// - usage >= 0.75 → Warning (yellow)
/// - otherwise → Normal (white/gray)
pub fn select_status_colors(usage_percentage: f32) -> StatusColors {
    if usage_percentage >= 0.9 {
        StatusColors::CRITICAL
    } else if usage_percentage >= 0.75 {
        StatusColors::WARNING
    } else {
        StatusColors::NORMAL
    }
}

/// Update persistent status line at bottom of terminal
pub fn update_status_line(agent: &Agent) {
    // Get terminal size
    let Ok((width, height)) = terminal::size() else {
        return; // Can't display status without terminal size
    };

    let mut stdout = stdout();

    // Get context usage from conversation manager
    let usage = agent.get_context_usage();
    let tokens_k = usage.context_tokens as f32 / 1000.0;
    let percentage = (usage.usage_percentage * 100.0) as u32;

    // Color code based on usage
    let colors = select_status_colors(usage.usage_percentage);

    let status_text = format!(
        "  Context: {:.1}k / {}k ({:>3}%) · {} messages",
        tokens_k,
        usage.max_context_tokens / 1000,
        percentage,
        usage.total_messages
    );

    // Save cursor position
    let _ = stdout.queue(cursor::SavePosition);

    // Move to bottom line
    let _ = stdout.queue(cursor::MoveTo(0, height - 1));

    // Print with background color spanning full width
    let _ = write!(stdout, "{}{}", colors.bg, colors.fg);
    let _ = write!(stdout, "{}", status_text);

    // Fill rest of line with background color
    let padding = (width as usize).saturating_sub(status_text.len());
    if padding > 0 {
        let _ = write!(stdout, "{}", " ".repeat(padding));
    }

    // Reset colors
    let _ = write!(stdout, "\x1b[0m");

    // Restore cursor position
    let _ = stdout.queue(cursor::RestorePosition);

    let _ = stdout.flush();
}

/// Clear the persistent status line
pub fn clear_status_line() {
    if let Ok((_, height)) = terminal::size() {
        let mut stdout = stdout();
        let _ = stdout.queue(cursor::SavePosition);
        let _ = stdout.queue(cursor::MoveTo(0, height - 1));
        let _ = stdout.execute(terminal::Clear(ClearType::CurrentLine));
        let _ = stdout.queue(cursor::RestorePosition);
        let _ = stdout.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod status_colors_tests {
        use super::*;

        #[test]
        fn critical_colors_are_red() {
            assert_eq!(StatusColors::CRITICAL.fg, "\x1b[31m");
            assert_eq!(StatusColors::CRITICAL.bg, "\x1b[48;5;52m");
        }

        #[test]
        fn warning_colors_are_yellow() {
            assert_eq!(StatusColors::WARNING.fg, "\x1b[33m");
            assert_eq!(StatusColors::WARNING.bg, "\x1b[48;5;58m");
        }

        #[test]
        fn normal_colors_are_white_gray() {
            assert_eq!(StatusColors::NORMAL.fg, "\x1b[37m");
            assert_eq!(StatusColors::NORMAL.bg, "\x1b[48;5;236m");
        }
    }

    mod select_status_colors_tests {
        use super::*;

        #[test]
        fn critical_at_90_percent() {
            assert_eq!(select_status_colors(0.9), StatusColors::CRITICAL);
            assert_eq!(select_status_colors(0.95), StatusColors::CRITICAL);
            assert_eq!(select_status_colors(1.0), StatusColors::CRITICAL);
        }

        #[test]
        fn warning_at_75_percent() {
            assert_eq!(select_status_colors(0.75), StatusColors::WARNING);
            assert_eq!(select_status_colors(0.80), StatusColors::WARNING);
            assert_eq!(select_status_colors(0.89), StatusColors::WARNING);
        }

        #[test]
        fn normal_below_75_percent() {
            assert_eq!(select_status_colors(0.0), StatusColors::NORMAL);
            assert_eq!(select_status_colors(0.5), StatusColors::NORMAL);
            assert_eq!(select_status_colors(0.74), StatusColors::NORMAL);
        }

        #[test]
        fn boundary_at_exactly_75() {
            // 0.75 should be WARNING (>= 0.75)
            assert_eq!(select_status_colors(0.75), StatusColors::WARNING);
        }

        #[test]
        fn boundary_just_below_75() {
            assert_eq!(select_status_colors(0.749999), StatusColors::NORMAL);
        }

        #[test]
        fn boundary_at_exactly_90() {
            // 0.9 should be CRITICAL (>= 0.9)
            assert_eq!(select_status_colors(0.9), StatusColors::CRITICAL);
        }

        #[test]
        fn boundary_just_below_90() {
            assert_eq!(select_status_colors(0.899999), StatusColors::WARNING);
        }
    }
}
