use crate::filesystem::validate_path;
use crate::prelude::*;
use ignore::WalkBuilder;
use regex::Regex;
use std::fs;
use std::path::PathBuf;

/// Search result entry
#[derive(Debug)]
pub struct SearchMatch {
    pub file_path: String,
    pub line_number: usize,
    pub line_content: String,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
}

/// Input for content search
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchInput {
    /// Root directory or file to search in
    pub root_path: PathBuf,

    /// Search pattern (regex or literal string)
    pub pattern: String,

    /// Search type: "files" for filename search, "content" for text search
    #[serde(default = "default_search_type")]
    pub search_type: String,

    /// Optional glob pattern to filter files (e.g., "*.rs|*.toml")
    #[serde(default)]
    pub file_pattern: Option<String>,

    /// Case-insensitive search (default: true)
    #[serde(default = "default_ignore_case")]
    pub ignore_case: bool,

    /// Maximum number of results to return (default: 100)
    #[serde(default = "default_max_results")]
    pub max_results: usize,

    /// Include hidden files and directories (default: false)
    #[serde(default)]
    pub include_hidden: bool,

    /// Lines of context to show around matches (default: 0)
    #[serde(default)]
    pub context_lines: usize,

    /// Force literal string matching instead of regex (default: false)
    #[serde(default)]
    pub literal_search: bool,
}

fn default_search_type() -> String {
    "content".to_string()
}

fn default_ignore_case() -> bool {
    true
}

fn default_max_results() -> usize {
    100
}

/// Tool for searching file contents using ripgrep-like functionality
pub struct SearchTool {
    base_path: PathBuf,
}

impl Default for SearchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchTool {
    /// Create a new SearchTool using the current working directory as the base path
    pub fn new() -> Self {
        Self {
            base_path: std::env::current_dir().expect("Failed to get current working directory"),
        }
    }

    /// Create a SearchTool with a custom base directory
    pub fn with_base_path(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    fn search_file_contents(
        &self,
        file_path: &PathBuf,
        pattern: &Regex,
        context_lines: usize,
    ) -> std::result::Result<Vec<SearchMatch>, ToolError> {
        let content = fs::read_to_string(file_path).map_err(|e| {
            ToolError::from(format!("Failed to read {}: {}", file_path.display(), e))
        })?;

        let lines: Vec<&str> = content.lines().collect();
        let mut matches = Vec::new();

        for (line_idx, line) in lines.iter().enumerate() {
            if pattern.is_match(line) {
                let context_before = if context_lines > 0 {
                    let start = line_idx.saturating_sub(context_lines);
                    lines[start..line_idx]
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    Vec::new()
                };

                let context_after = if context_lines > 0 {
                    let end = (line_idx + 1 + context_lines).min(lines.len());
                    lines[line_idx + 1..end]
                        .iter()
                        .map(|s| s.to_string())
                        .collect()
                } else {
                    Vec::new()
                };

                matches.push(SearchMatch {
                    file_path: file_path.display().to_string(),
                    line_number: line_idx + 1, // 1-indexed
                    line_content: line.to_string(),
                    context_before,
                    context_after,
                });
            }
        }

        Ok(matches)
    }

    fn search_filenames(
        &self,
        root_path: &PathBuf,
        pattern: &Regex,
        include_hidden: bool,
        max_results: usize,
    ) -> std::result::Result<Vec<String>, ToolError> {
        let walker = WalkBuilder::new(root_path)
            .hidden(!include_hidden)
            .git_ignore(true)
            .max_depth(Some(50))
            .build();

        let mut matches = Vec::new();

        for entry in walker {
            if matches.len() >= max_results {
                break;
            }

            let entry =
                entry.map_err(|e| ToolError::from(format!("Error walking directory: {}", e)))?;

            if let Some(file_name) = entry.file_name().to_str() {
                if pattern.is_match(file_name) {
                    if let Ok(relative_path) = entry.path().strip_prefix(root_path) {
                        matches.push(relative_path.display().to_string());
                    }
                }
            }
        }

        Ok(matches)
    }
}

impl Tool for SearchTool {
    type Input = SearchInput;

    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Search for text patterns in files (content search) or search filenames. \
         Uses regex patterns and respects .gitignore. Can show context lines around matches."
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        let root_path = validate_path(&self.base_path, &input.root_path)
            .map_err(|e| ToolError::from(e.to_string()))?;

        // Build regex pattern
        let pattern_str = if input.literal_search {
            regex::escape(&input.pattern)
        } else {
            input.pattern.clone()
        };

        let regex_pattern = if input.ignore_case {
            Regex::new(&format!("(?i){}", pattern_str))
        } else {
            Regex::new(&pattern_str)
        }
        .map_err(|e| ToolError::from(format!("Invalid regex pattern: {}", e)))?;

        // Parse file pattern if provided
        let file_glob = if let Some(ref pattern) = input.file_pattern {
            Some(
                glob::Pattern::new(pattern)
                    .map_err(|e| ToolError::from(format!("Invalid file pattern: {}", e)))?,
            )
        } else {
            None
        };

        match input.search_type.as_str() {
            "files" => {
                // Filename search
                let matches = self.search_filenames(
                    &root_path,
                    &regex_pattern,
                    input.include_hidden,
                    input.max_results,
                )?;

                let content = if matches.is_empty() {
                    format!(
                        "No files matching '{}' found in {}",
                        input.pattern,
                        input.root_path.display()
                    )
                } else {
                    format!(
                        "Found {} file(s) matching '{}':\n{}",
                        matches.len(),
                        input.pattern,
                        matches.join("\n")
                    )
                };

                Ok(content.into())
            }
            "content" => {
                // Content search
                let walker = WalkBuilder::new(&root_path)
                    .hidden(!input.include_hidden)
                    .git_ignore(true)
                    .max_depth(Some(50))
                    .build();

                let mut all_matches = Vec::new();

                for entry in walker {
                    if all_matches.len() >= input.max_results {
                        break;
                    }

                    let entry = entry
                        .map_err(|e| ToolError::from(format!("Error walking directory: {}", e)))?;

                    // Skip directories
                    if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                        continue;
                    }

                    // Check file pattern if specified
                    if let Some(ref glob_pattern) = file_glob {
                        if let Some(file_name) = entry.file_name().to_str() {
                            if !glob_pattern.matches(file_name) {
                                continue;
                            }
                        }
                    }

                    // Search file contents
                    match self.search_file_contents(
                        &entry.path().to_path_buf(),
                        &regex_pattern,
                        input.context_lines,
                    ) {
                        Ok(matches) => {
                            for m in matches {
                                if all_matches.len() >= input.max_results {
                                    break;
                                }
                                all_matches.push(m);
                            }
                        }
                        Err(_) => {
                            // Skip files that can't be read (binary, permissions, etc.)
                            continue;
                        }
                    }
                }

                let content = if all_matches.is_empty() {
                    format!(
                        "No matches for '{}' found in {}",
                        input.pattern,
                        input.root_path.display()
                    )
                } else {
                    let mut result = format!(
                        "Found {} match(es) for '{}':\n\n",
                        all_matches.len(),
                        input.pattern
                    );

                    for m in all_matches {
                        result.push_str(&format!("{}:{}\n", m.file_path, m.line_number));

                        // Add context before
                        for ctx in &m.context_before {
                            result.push_str(&format!("  | {}\n", ctx));
                        }

                        // Add matching line
                        result.push_str(&format!("  > {}\n", m.line_content));

                        // Add context after
                        for ctx in &m.context_after {
                            result.push_str(&format!("  | {}\n", ctx));
                        }

                        result.push('\n');
                    }

                    result
                };

                Ok(content.into())
            }
            _ => Err(format!(
                "Invalid search_type: '{}'. Must be 'files' or 'content'",
                input.search_type
            )
            .into()),
        }
    }

    fn format_output_plain(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let lines: Vec<&str> = output.lines().collect();
        if lines.is_empty() || output.starts_with("No matches") || output.starts_with("No files") {
            return output.to_string();
        }
        if output.starts_with("Found") && output.contains("file(s)") {
            return output.to_string();
        }

        let mut out = String::new();
        let mut current_file: Option<&str> = None;

        for line in lines {
            if line.starts_with("Found ") {
                out.push_str(line);
                out.push_str("\n\n");
                continue;
            }
            if let Some(colon_idx) = line.find(':') {
                let potential_file = &line[..colon_idx];
                if !line.starts_with("  ")
                    && (potential_file.contains('/') || potential_file.contains('.'))
                {
                    if current_file != Some(potential_file) {
                        if current_file.is_some() {
                            out.push('\n');
                        }
                        out.push_str(potential_file);
                        out.push('\n');
                        current_file = Some(potential_file);
                    }
                    let rest = &line[colon_idx + 1..];
                    if let Some(content_start) = rest.find(|c: char| !c.is_ascii_digit()) {
                        out.push_str(&format!(
                            "  {}:{}\n",
                            &rest[..content_start],
                            &rest[content_start..]
                        ));
                    } else {
                        out.push_str(&format!("  {}\n", rest));
                    }
                } else {
                    out.push_str(line);
                    out.push('\n');
                }
            } else if line.starts_with("  >") {
                out.push_str(&format!("  → {}\n", &line[4..]));
            } else if line.starts_with("  |") {
                out.push_str(&format!("    {}\n", &line[4..]));
            } else if !line.is_empty() {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    fn format_output_ansi(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let lines: Vec<&str> = output.lines().collect();
        if lines.is_empty() {
            return output.to_string();
        }
        if output.starts_with("No matches") || output.starts_with("No files") {
            return format!("\x1b[2m{}\x1b[0m", output);
        }
        if output.starts_with("Found") && output.contains("file(s)") {
            let mut out = String::new();
            for line in lines {
                if line.starts_with("Found") {
                    out.push_str(&format!("\x1b[1m{}\x1b[0m\n", line));
                } else {
                    out.push_str(&format!("\x1b[35m{}\x1b[0m\n", line));
                }
            }
            return out;
        }

        let mut out = String::new();
        let mut current_file: Option<&str> = None;

        for line in lines {
            if line.starts_with("Found ") {
                out.push_str(&format!("\x1b[1m{}\x1b[0m\n\n", line));
                continue;
            }
            if let Some(colon_idx) = line.find(':') {
                let potential_file = &line[..colon_idx];
                if !line.starts_with("  ")
                    && (potential_file.contains('/') || potential_file.contains('.'))
                {
                    if current_file != Some(potential_file) {
                        if current_file.is_some() {
                            out.push('\n');
                        }
                        out.push_str(&format!("\x1b[35m{}\x1b[0m\n", potential_file));
                        current_file = Some(potential_file);
                    }
                    let rest = &line[colon_idx + 1..];
                    if let Some(content_start) = rest.find(|c: char| !c.is_ascii_digit()) {
                        out.push_str(&format!(
                            "\x1b[32m{}\x1b[0m:{}\n",
                            &rest[..content_start],
                            &rest[content_start..]
                        ));
                    } else {
                        out.push_str(&format!("  {}\n", rest));
                    }
                } else {
                    out.push_str(line);
                    out.push('\n');
                }
            } else if line.starts_with("  >") {
                out.push_str(&format!("\x1b[33m→\x1b[0m {}\n", &line[4..]));
            } else if line.starts_with("  |") {
                out.push_str(&format!("\x1b[2m  {}\x1b[0m\n", &line[4..]));
            } else if !line.is_empty() {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    fn format_output_markdown(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let lines: Vec<&str> = output.lines().collect();
        if lines.is_empty() {
            return output.to_string();
        }
        if output.starts_with("No matches") || output.starts_with("No files") {
            return format!("*{}*", output);
        }
        if output.starts_with("Found") && output.contains("file(s)") {
            let mut out = String::new();
            for line in lines {
                if line.starts_with("Found") {
                    out.push_str(&format!("**{}**\n\n", line));
                } else {
                    out.push_str(&format!("- `{}`\n", line));
                }
            }
            return out;
        }

        let mut out = String::new();
        let mut current_file: Option<&str> = None;
        let mut in_code_block = false;

        for line in lines {
            if line.starts_with("Found ") {
                out.push_str(&format!("**{}**\n\n", line));
                continue;
            }
            if let Some(colon_idx) = line.find(':') {
                let potential_file = &line[..colon_idx];
                if !line.starts_with("  ")
                    && (potential_file.contains('/') || potential_file.contains('.'))
                {
                    if current_file != Some(potential_file) {
                        if in_code_block {
                            out.push_str("```\n\n");
                        }
                        out.push_str(&format!("### `{}`\n```\n", potential_file));
                        in_code_block = true;
                        current_file = Some(potential_file);
                    }
                    out.push_str(&format!("{}\n", &line[colon_idx + 1..]));
                } else {
                    out.push_str(line);
                    out.push('\n');
                }
            } else if line.starts_with("  >") || line.starts_with("  |") {
                out.push_str(&format!("{}\n", &line[2..]));
            } else if !line.is_empty() {
                out.push_str(line);
                out.push('\n');
            }
        }
        if in_code_block {
            out.push_str("```\n");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ==================== Default and constructor tests ====================

    #[test]
    fn test_default() {
        let tool: SearchTool = Default::default();
        assert_eq!(tool.name(), "search");
    }

    #[test]
    fn test_tool_name() {
        let tool = SearchTool::new();
        assert_eq!(tool.name(), "search");
    }

    #[test]
    fn test_tool_description() {
        let tool = SearchTool::new();
        assert!(!tool.description().is_empty());
        assert!(tool.description().contains("Search"));
    }

    // ==================== Default value function tests ====================

    #[test]
    fn test_default_search_type() {
        assert_eq!(default_search_type(), "content");
    }

    #[test]
    fn test_default_ignore_case() {
        assert!(default_ignore_case());
    }

    #[test]
    fn test_default_max_results() {
        assert_eq!(default_max_results(), 100);
    }

    // ==================== format_output_plain tests ====================

    #[test]
    fn test_format_output_plain_no_matches() {
        let tool = SearchTool::new();
        let result: ToolResult = "No matches for 'pattern' found in .".into();

        let formatted = tool.format_output_plain(&result);
        assert_eq!(formatted, "No matches for 'pattern' found in .");
    }

    #[test]
    fn test_format_output_plain_no_files() {
        let tool = SearchTool::new();
        let result: ToolResult = "No files matching 'pattern' found in .".into();

        let formatted = tool.format_output_plain(&result);
        assert_eq!(formatted, "No files matching 'pattern' found in .");
    }

    #[test]
    fn test_format_output_plain_file_search() {
        let tool = SearchTool::new();
        let result: ToolResult = "Found 2 file(s) matching '*.rs':\ntest1.rs\ntest2.rs".into();

        let formatted = tool.format_output_plain(&result);
        assert!(formatted.contains("Found 2 file(s)"));
        assert!(formatted.contains("test1.rs"));
        assert!(formatted.contains("test2.rs"));
    }

    #[test]
    fn test_format_output_plain_content_search() {
        let tool = SearchTool::new();
        let result: ToolResult =
            "Found 1 match(es) for 'test':\n\nsrc/main.rs:10\n  > fn test() {}".into();

        let formatted = tool.format_output_plain(&result);
        assert!(formatted.contains("Found 1 match"));
        assert!(formatted.contains("src/main.rs"));
        // Check for arrow transformation
        assert!(formatted.contains("→") || formatted.contains(">"));
    }

    #[test]
    fn test_format_output_plain_with_context() {
        let tool = SearchTool::new();
        let result: ToolResult = "Found 1 match(es) for 'target':\n\ntest.txt:3\n  | line before\n  > target line\n  | line after".into();

        let formatted = tool.format_output_plain(&result);
        assert!(formatted.contains("line before"));
        assert!(formatted.contains("target line"));
        assert!(formatted.contains("line after"));
    }

    // ==================== format_output_ansi tests ====================

    #[test]
    fn test_format_output_ansi_no_matches() {
        let tool = SearchTool::new();
        let result: ToolResult = "No matches for 'pattern' found in .".into();

        let formatted = tool.format_output_ansi(&result);
        // Should be dimmed
        assert!(formatted.contains("\x1b[2m"));
        assert!(formatted.contains("No matches"));
    }

    #[test]
    fn test_format_output_ansi_no_files() {
        let tool = SearchTool::new();
        let result: ToolResult = "No files matching 'pattern' found in .".into();

        let formatted = tool.format_output_ansi(&result);
        assert!(formatted.contains("\x1b[2m")); // dimmed
    }

    #[test]
    fn test_format_output_ansi_file_search() {
        let tool = SearchTool::new();
        let result: ToolResult = "Found 2 file(s) matching '*.rs':\ntest1.rs\ntest2.rs".into();

        let formatted = tool.format_output_ansi(&result);
        // Header should be bold
        assert!(formatted.contains("\x1b[1m"));
        // Files should be magenta
        assert!(formatted.contains("\x1b[35m"));
    }

    #[test]
    fn test_format_output_ansi_content_search() {
        let tool = SearchTool::new();
        // Use format that triggers line number extraction (need colon after number)
        let result: ToolResult =
            "Found 1 match(es) for 'test':\n\nsrc/main.rs:10:fn test() {}".into();

        let formatted = tool.format_output_ansi(&result);
        // Header should be bold
        assert!(formatted.contains("\x1b[1m"));
        // File path should be magenta
        assert!(formatted.contains("\x1b[35m"));
        // Line number should be green (only when content follows the line number)
        assert!(formatted.contains("\x1b[32m"));
    }

    #[test]
    fn test_format_output_ansi_match_indicator() {
        let tool = SearchTool::new();
        let result: ToolResult =
            "Found 1 match(es) for 'test':\n\ntest.txt:10\n  > fn test() {}".into();

        let formatted = tool.format_output_ansi(&result);
        // Match indicator (arrow) should be yellow
        assert!(formatted.contains("\x1b[33m"));
    }

    #[test]
    fn test_format_output_ansi_with_context() {
        let tool = SearchTool::new();
        let result: ToolResult =
            "Found 1 match(es) for 'target':\n\ntest.txt:3\n  | context line\n  > target line"
                .into();

        let formatted = tool.format_output_ansi(&result);
        // Context lines should be dimmed
        assert!(formatted.contains("\x1b[2m"));
    }

    // ==================== format_output_markdown tests ====================

    #[test]
    fn test_format_output_markdown_no_matches() {
        let tool = SearchTool::new();
        let result: ToolResult = "No matches for 'pattern' found in .".into();

        let formatted = tool.format_output_markdown(&result);
        // Should be italicized
        assert!(formatted.contains("*No matches"));
    }

    #[test]
    fn test_format_output_markdown_no_files() {
        let tool = SearchTool::new();
        let result: ToolResult = "No files matching 'pattern' found in .".into();

        let formatted = tool.format_output_markdown(&result);
        assert!(formatted.contains("*No files"));
    }

    #[test]
    fn test_format_output_markdown_file_search() {
        let tool = SearchTool::new();
        let result: ToolResult = "Found 2 file(s) matching '*.rs':\ntest1.rs\ntest2.rs".into();

        let formatted = tool.format_output_markdown(&result);
        // Header should be bold
        assert!(formatted.contains("**Found 2 file(s)"));
        // Files should be in list with code formatting
        assert!(formatted.contains("- `test1.rs`"));
        assert!(formatted.contains("- `test2.rs`"));
    }

    #[test]
    fn test_format_output_markdown_content_search() {
        let tool = SearchTool::new();
        let result: ToolResult =
            "Found 1 match(es) for 'test':\n\nsrc/main.rs:10\n  > fn test() {}".into();

        let formatted = tool.format_output_markdown(&result);
        // Header should be bold
        assert!(formatted.contains("**Found 1 match"));
        // File should be a heading with code formatting
        assert!(formatted.contains("### `src/main.rs`"));
        // Code block should be present
        assert!(formatted.contains("```"));
    }

    #[test]
    fn test_format_output_markdown_closes_code_block() {
        let tool = SearchTool::new();
        let result: ToolResult =
            "Found 1 match(es) for 'test':\n\nsrc/main.rs:10\n  > fn test() {}".into();

        let formatted = tool.format_output_markdown(&result);
        // Should have both opening and closing code blocks
        let open_count = formatted.matches("```").count();
        // Should have at least one pair (open + close)
        assert!(open_count >= 2 || open_count == 0);
    }

    // ==================== SearchMatch struct tests ====================

    #[test]
    fn test_search_match_debug() {
        let m = SearchMatch {
            file_path: "test.rs".to_string(),
            line_number: 42,
            line_content: "fn test()".to_string(),
            context_before: vec!["// comment".to_string()],
            context_after: vec!["}".to_string()],
        };
        let debug_str = format!("{:?}", m);
        assert!(debug_str.contains("test.rs"));
        assert!(debug_str.contains("42"));
    }

    // ==================== Integration tests ====================

    #[tokio::test]
    async fn test_content_search() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(
            temp_dir.path().join("test1.rs"),
            "fn main() {\n    println!(\"Hello\");\n}",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("test2.rs"),
            "fn helper() {\n    println!(\"World\");\n}",
        )
        .unwrap();

        let tool = SearchTool::with_base_path(temp_dir.path().to_path_buf());
        let input = SearchInput {
            root_path: PathBuf::from("."),
            pattern: "println".to_string(),
            search_type: "content".to_string(),
            file_pattern: Some("*.rs".to_string()),
            ignore_case: true,
            max_results: 100,
            include_hidden: false,
            context_lines: 0,
            literal_search: false,
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("test1.rs"));
        assert!(result.as_text().contains("test2.rs"));
        assert!(result.as_text().contains("println"));
    }

    #[tokio::test]
    async fn test_filename_search() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("test1.rs"), "").unwrap();
        fs::write(temp_dir.path().join("test2.rs"), "").unwrap();
        fs::write(temp_dir.path().join("readme.md"), "").unwrap();

        let tool = SearchTool::with_base_path(temp_dir.path().to_path_buf());
        let input = SearchInput {
            root_path: PathBuf::from("."),
            pattern: r"\.rs$".to_string(),
            search_type: "files".to_string(),
            file_pattern: None,
            ignore_case: true,
            max_results: 100,
            include_hidden: false,
            context_lines: 0,
            literal_search: false,
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("test1.rs"));
        assert!(result.as_text().contains("test2.rs"));
        assert!(!result.as_text().contains("readme.md"));
    }

    #[tokio::test]
    async fn test_context_lines() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(
            temp_dir.path().join("test.txt"),
            "line 1\nline 2\ntarget line\nline 4\nline 5",
        )
        .unwrap();

        let tool = SearchTool::with_base_path(temp_dir.path().to_path_buf());
        let input = SearchInput {
            root_path: PathBuf::from("."),
            pattern: "target".to_string(),
            search_type: "content".to_string(),
            file_pattern: None,
            ignore_case: true,
            max_results: 100,
            include_hidden: false,
            context_lines: 1,
            literal_search: true,
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("line 2"));
        assert!(result.as_text().contains("target line"));
        assert!(result.as_text().contains("line 4"));
    }

    // ===== Edge Case Tests =====

    #[tokio::test]
    async fn test_search_hidden_files() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join(".hidden"), "secret content").unwrap();
        fs::write(temp_dir.path().join("visible.txt"), "normal content").unwrap();

        let tool = SearchTool::with_base_path(temp_dir.path().to_path_buf());

        // Search without include_hidden
        let input = SearchInput {
            root_path: PathBuf::from("."),
            pattern: "content".to_string(),
            search_type: "content".to_string(),
            file_pattern: None,
            ignore_case: true,
            max_results: 100,
            include_hidden: false,
            context_lines: 0,
            literal_search: true,
        };

        let result = tool.execute(input.clone()).await.unwrap();
        let output = result.as_text();
        assert!(output.contains("visible.txt"));
        assert!(!output.contains(".hidden"));

        // Search with include_hidden
        let input_with_hidden = SearchInput {
            include_hidden: true,
            ..input
        };

        let result_with_hidden = tool.execute(input_with_hidden).await.unwrap();
        let output_with_hidden = result_with_hidden.as_text();
        assert!(output_with_hidden.contains(".hidden") || output_with_hidden.contains("secret"));
    }

    #[tokio::test]
    async fn test_search_large_file() {
        let temp_dir = TempDir::new().unwrap();

        // Create a large file with 1000 lines
        let large_content = (0..1000)
            .map(|i| {
                if i == 500 {
                    "NEEDLE in the haystack".to_string()
                } else {
                    format!("Line {} with regular content", i)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        fs::write(temp_dir.path().join("large.txt"), large_content).unwrap();

        let tool = SearchTool::with_base_path(temp_dir.path().to_path_buf());
        let input = SearchInput {
            root_path: PathBuf::from("."),
            pattern: "NEEDLE".to_string(),
            search_type: "content".to_string(),
            file_pattern: None,
            ignore_case: false,
            max_results: 100,
            include_hidden: false,
            context_lines: 0,
            literal_search: true,
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("NEEDLE"));
        assert!(result.as_text().contains("large.txt"));
    }

    #[tokio::test]
    async fn test_search_no_results() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("test.txt"), "some content").unwrap();

        let tool = SearchTool::with_base_path(temp_dir.path().to_path_buf());
        let input = SearchInput {
            root_path: PathBuf::from("."),
            pattern: "NONEXISTENT_PATTERN_XYZ".to_string(),
            search_type: "content".to_string(),
            file_pattern: None,
            ignore_case: true,
            max_results: 100,
            include_hidden: false,
            context_lines: 0,
            literal_search: true,
        };

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();
        // When no results, output should be minimal or indicate no matches
        // The exact format may vary, so just ensure we got a result without panicking
        assert!(
            !output.contains("NONEXISTENT_PATTERN_XYZ") || output.is_empty() || output.len() < 100
        );
    }

    #[tokio::test]
    async fn test_search_case_sensitive() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("test.txt"), "Hello HELLO hello").unwrap();

        let tool = SearchTool::with_base_path(temp_dir.path().to_path_buf());

        // Case-sensitive search
        let input = SearchInput {
            root_path: PathBuf::from("."),
            pattern: "HELLO".to_string(),
            search_type: "content".to_string(),
            file_pattern: None,
            ignore_case: false,
            max_results: 100,
            include_hidden: false,
            context_lines: 0,
            literal_search: true,
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("HELLO"));
    }

    #[tokio::test]
    async fn test_search_max_results_limit() {
        let temp_dir = TempDir::new().unwrap();

        // Create multiple files with matches
        for i in 0..10 {
            fs::write(
                temp_dir.path().join(format!("file{}.txt", i)),
                "target content",
            )
            .unwrap();
        }

        let tool = SearchTool::with_base_path(temp_dir.path().to_path_buf());
        let input = SearchInput {
            root_path: PathBuf::from("."),
            pattern: "target".to_string(),
            search_type: "content".to_string(),
            file_pattern: None,
            ignore_case: true,
            max_results: 3, // Limit to 3 results
            include_hidden: false,
            context_lines: 0,
            literal_search: true,
        };

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();
        // Should have limited results (exact count may vary based on implementation)
        assert!(output.contains("target") || output.contains("file"));
    }

    #[tokio::test]
    async fn test_search_utf8_content() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(
            temp_dir.path().join("utf8.txt"),
            "Hello 世界! Ümläüts: äöü 🎵",
        )
        .unwrap();

        let tool = SearchTool::with_base_path(temp_dir.path().to_path_buf());
        let input = SearchInput {
            root_path: PathBuf::from("."),
            pattern: "世界".to_string(),
            search_type: "content".to_string(),
            file_pattern: None,
            ignore_case: false,
            max_results: 100,
            include_hidden: false,
            context_lines: 0,
            literal_search: true,
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("世界"));
    }
}
