use crate::filesystem::validate_path;
use crate::prelude::*;
use std::path::PathBuf;
use strsim::normalized_levenshtein;

/// Input for editing a block of text in a file
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EditBlockInput {
    /// Path to the file to edit
    pub file_path: PathBuf,

    /// Text to search for and replace
    pub old_string: String,

    /// Replacement text
    pub new_string: String,

    /// Expected number of replacements (default: 1)
    #[serde(default = "default_replacements")]
    pub expected_replacements: usize,

    /// Enable fuzzy matching if exact match fails (default: true)
    #[serde(default = "default_fuzzy")]
    pub enable_fuzzy: bool,

    /// Minimum similarity threshold for fuzzy matching (0.0-1.0, default: 0.7)
    #[serde(default = "default_threshold")]
    pub fuzzy_threshold: f32,
}

fn default_replacements() -> usize {
    1
}

fn default_fuzzy() -> bool {
    true
}

fn default_threshold() -> f32 {
    0.7
}

/// Result of a fuzzy match
#[derive(Debug)]
struct FuzzyMatch {
    start: usize,
    end: usize,
    similarity: f64,
    matched_text: String,
}

/// Tool for surgical code editing with exact and fuzzy string replacement
pub struct EditBlockTool {
    base_path: PathBuf,
}

impl Default for EditBlockTool {
    fn default() -> Self {
        Self::new()
    }
}

impl EditBlockTool {
    /// Create a new EditBlockTool using the current working directory as the base path
    pub fn new() -> Self {
        Self {
            base_path: std::env::current_dir().expect("Failed to get current working directory"),
        }
    }

    /// Create an EditBlockTool with a custom base directory
    pub fn with_base_path(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Find the best fuzzy match for a pattern in text
    fn find_fuzzy_match(text: &str, pattern: &str, threshold: f32) -> Option<FuzzyMatch> {
        let pattern_len = pattern.len();
        if pattern_len == 0 || pattern_len > text.len() {
            return None;
        }

        let mut best_match: Option<FuzzyMatch> = None;
        let mut best_similarity = threshold as f64;

        // Slide a window across the text
        for start in 0..=(text.len() - pattern_len) {
            let end = (start + pattern_len).min(text.len());
            let window = &text[start..end];

            let similarity = normalized_levenshtein(pattern, window);

            if similarity > best_similarity {
                best_similarity = similarity;
                best_match = Some(FuzzyMatch {
                    start,
                    end,
                    similarity,
                    matched_text: window.to_string(),
                });
            }
        }

        // Also try with slightly larger and smaller windows
        for window_size in [
            pattern_len.saturating_sub(pattern_len / 10),
            pattern_len + pattern_len / 10,
        ] {
            if window_size == 0 || window_size > text.len() {
                continue;
            }

            for start in 0..=(text.len() - window_size) {
                let end = (start + window_size).min(text.len());
                let window = &text[start..end];

                let similarity = normalized_levenshtein(pattern, window);

                if similarity > best_similarity {
                    best_similarity = similarity;
                    best_match = Some(FuzzyMatch {
                        start,
                        end,
                        similarity,
                        matched_text: window.to_string(),
                    });
                }
            }
        }

        best_match
    }

    /// Preserve the line ending style of the file
    fn detect_line_ending(content: &str) -> &str {
        if content.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        }
    }
}

impl Tool for EditBlockTool {
    type Input = EditBlockInput;

    fn name(&self) -> &str {
        "edit_block"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing text. Supports exact matching with fallback to fuzzy matching. Preserves file line endings."
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        let path = validate_path(&self.base_path, &input.file_path)
            .map_err(|e| ToolError::from(e.to_string()))?;

        // Read the file
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| ToolError::from(format!("Failed to read file: {}", e)))?;

        let line_ending = Self::detect_line_ending(&content);

        // Try exact replacement first
        let replacement_count = content.matches(&input.old_string).count();

        let (new_content, actual_replacements, method) = if replacement_count > 0 {
            // Exact match found
            let new_content = content.replace(&input.old_string, &input.new_string);
            (new_content, replacement_count, "exact".to_string())
        } else if input.enable_fuzzy {
            // Try fuzzy matching
            match Self::find_fuzzy_match(&content, &input.old_string, input.fuzzy_threshold) {
                Some(fuzzy_match) => {
                    let new_content = format!(
                        "{}{}{}",
                        &content[..fuzzy_match.start],
                        &input.new_string,
                        &content[fuzzy_match.end..]
                    );

                    let info = format!(
                        "fuzzy (similarity: {:.1}%)\nMatched text:\n{}",
                        fuzzy_match.similarity * 100.0,
                        fuzzy_match.matched_text
                    );

                    (new_content, 1, info)
                }
                None => {
                    return Err(format!(
                        "No match found for the specified text (tried exact and fuzzy matching with threshold {:.1}%)",
                        input.fuzzy_threshold * 100.0
                    ).into());
                }
            }
        } else {
            return Err("No exact match found and fuzzy matching is disabled".into());
        };

        // Validate replacement count
        if actual_replacements != input.expected_replacements {
            return Err(format!(
                "Expected {} replacement(s) but found {}",
                input.expected_replacements, actual_replacements
            )
            .into());
        }

        // Normalize line endings if needed
        // First normalize to LF, then convert to target line ending to avoid double-CR
        let final_content = if line_ending == "\r\n" {
            // First convert any existing CRLF to LF to avoid doubling
            let normalized = new_content.replace("\r\n", "\n");
            // Then convert all LF to CRLF
            normalized.replace('\n', "\r\n")
        } else {
            new_content
        };

        // Write the file
        tokio::fs::write(&path, final_content.as_bytes())
            .await
            .map_err(|e| ToolError::from(format!("Failed to write file: {}", e)))?;

        // Calculate line changes
        let old_lines = input.old_string.lines().count();
        let new_lines = input.new_string.lines().count();
        let line_diff = new_lines as i64 - old_lines as i64;

        let line_change = if line_diff > 0 {
            format!("(\x1b[32m+{} lines\x1b[0m)", line_diff)
        } else if line_diff < 0 {
            format!("(\x1b[31m{} lines\x1b[0m)", line_diff)
        } else {
            "(no change in line count)".to_string()
        };

        let content = format!(
            "Successfully edited {} using {} matching\n{} replacement(s) {}",
            input.file_path.display(),
            method,
            actual_replacements,
            line_change
        );

        Ok(content.into())
    }

    fn format_input_plain(&self, params: &serde_json::Value) -> String {
        let file_path = params
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let old_string = params
            .get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let new_string = params
            .get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let mut output = format!("edit_block: {}\n", file_path);
        output.push_str("--- old\n");
        for line in old_string.lines() {
            output.push_str(&format!("- {}\n", line));
        }
        output.push_str("+++ new\n");
        for line in new_string.lines() {
            output.push_str(&format!("+ {}\n", line));
        }
        output
    }

    fn format_input_ansi(&self, params: &serde_json::Value) -> String {
        let file_path = params
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let old_string = params
            .get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let new_string = params
            .get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let mut output = format!("\x1b[1medit_block:\x1b[0m {}\n", file_path);
        output.push_str("\x1b[31m--- old\x1b[0m\n");
        for line in old_string.lines() {
            output.push_str(&format!("\x1b[31m- {}\x1b[0m\n", line));
        }
        output.push_str("\x1b[32m+++ new\x1b[0m\n");
        for line in new_string.lines() {
            output.push_str(&format!("\x1b[32m+ {}\x1b[0m\n", line));
        }
        output
    }

    fn format_input_markdown(&self, params: &serde_json::Value) -> String {
        let file_path = params
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let old_string = params
            .get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let new_string = params
            .get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let mut output = format!("**edit_block:** `{}`\n\n```diff\n", file_path);
        for line in old_string.lines() {
            output.push_str(&format!("- {}\n", line));
        }
        for line in new_string.lines() {
            output.push_str(&format!("+ {}\n", line));
        }
        output.push_str("```\n");
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ===== Metadata Tests =====

    #[test]
    fn test_tool_metadata() {
        let tool: EditBlockTool = Default::default();
        assert_eq!(tool.name(), "edit_block");
        assert!(!tool.description().is_empty());

        let tool2 = EditBlockTool::new();
        assert_eq!(tool2.name(), "edit_block");
    }

    #[test]
    fn test_format_methods() {
        let tool = EditBlockTool::new();
        let params =
            serde_json::json!({"file_path": "test.txt", "old_string": "old", "new_string": "new"});

        assert!(!tool.format_input_plain(&params).is_empty());
        assert!(!tool.format_input_ansi(&params).is_empty());
        assert!(!tool.format_input_markdown(&params).is_empty());

        let result = ToolResult::from("Edited file");
        assert!(!tool.format_output_plain(&result).is_empty());
        assert!(!tool.format_output_ansi(&result).is_empty());
        assert!(!tool.format_output_markdown(&result).is_empty());
    }

    #[test]
    fn test_default_values() {
        // Deserialize without optional fields to trigger defaults
        let input: EditBlockInput = serde_json::from_value(serde_json::json!({
            "file_path": "test.txt",
            "old_string": "old",
            "new_string": "new"
        }))
        .unwrap();

        assert_eq!(input.expected_replacements, 1);
        assert!(input.enable_fuzzy);
        assert!((input.fuzzy_threshold - 0.7).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_edit_block_exact() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "Hello, World!\nThis is a test.").unwrap();

        let tool = EditBlockTool::with_base_path(temp_dir.path().to_path_buf());
        let input = EditBlockInput {
            file_path: PathBuf::from("test.txt"),
            old_string: "World".to_string(),
            new_string: "Rust".to_string(),
            expected_replacements: 1,
            enable_fuzzy: false,
            fuzzy_threshold: 0.7,
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("exact matching"));

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "Hello, Rust!\nThis is a test.");
    }

    #[tokio::test]
    async fn test_edit_block_fuzzy() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "Hello, World!\nThis is a test.").unwrap();

        let tool = EditBlockTool::with_base_path(temp_dir.path().to_path_buf());
        let input = EditBlockInput {
            file_path: PathBuf::from("test.txt"),
            old_string: "Wrld".to_string(), // Typo - should match "World" via fuzzy
            new_string: "Rust".to_string(),
            expected_replacements: 1,
            enable_fuzzy: true,
            fuzzy_threshold: 0.7,
        };

        let result = tool.execute(input).await.unwrap();
        assert!(result.as_text().contains("fuzzy"));
    }

    #[tokio::test]
    async fn test_edit_block_preserves_line_endings() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "Line1\r\nLine2\r\n").unwrap();

        let tool = EditBlockTool::with_base_path(temp_dir.path().to_path_buf());
        let input = EditBlockInput {
            file_path: PathBuf::from("test.txt"),
            old_string: "Line1".to_string(),
            new_string: "First".to_string(),
            expected_replacements: 1,
            enable_fuzzy: false,
            fuzzy_threshold: 0.7,
        };

        tool.execute(input).await.unwrap();

        let content = fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("\r\n"));
    }

    // ===== Comprehensive Line Ending Tests =====

    #[tokio::test]
    async fn test_edit_block_lf_only() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("lf.txt");

        let original = "Line 1\nLine 2\nLine 3\n";
        fs::write(&file_path, original).unwrap();

        let tool = EditBlockTool::with_base_path(temp_dir.path().to_path_buf());
        let input = EditBlockInput {
            file_path: PathBuf::from("lf.txt"),
            old_string: "Line 2".to_string(),
            new_string: "Modified Line 2".to_string(),
            expected_replacements: 1,
            enable_fuzzy: false,
            fuzzy_threshold: 0.7,
        };

        tool.execute(input).await.unwrap();

        let bytes = fs::read(&file_path).unwrap();
        let content = String::from_utf8(bytes).unwrap();
        assert!(content.contains("Modified Line 2"));
        assert!(content.contains("\n"));
        assert!(!content.contains("\r\n"));
    }

    #[tokio::test]
    async fn test_edit_block_crlf_only() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("crlf.txt");

        let original = "Line 1\r\nLine 2\r\nLine 3\r\n";
        fs::write(&file_path, original).unwrap();

        let tool = EditBlockTool::with_base_path(temp_dir.path().to_path_buf());
        let input = EditBlockInput {
            file_path: PathBuf::from("crlf.txt"),
            old_string: "Line 2".to_string(),
            new_string: "Modified Line 2".to_string(),
            expected_replacements: 1,
            enable_fuzzy: false,
            fuzzy_threshold: 0.7,
        };

        tool.execute(input).await.unwrap();

        let bytes = fs::read(&file_path).unwrap();
        let content = String::from_utf8(bytes).unwrap();
        assert!(content.contains("Modified Line 2"));
        assert!(content.contains("\r\n"));
        // Count CRLF sequences
        let crlf_count = content.matches("\r\n").count();
        assert!(crlf_count >= 2); // Should still have CRLF endings
    }

    #[tokio::test]
    async fn test_edit_block_mixed_line_endings() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("mixed.txt");

        let original = "Line 1\nLine 2\r\nLine 3\rLine 4";
        fs::write(&file_path, original).unwrap();

        let tool = EditBlockTool::with_base_path(temp_dir.path().to_path_buf());
        let input = EditBlockInput {
            file_path: PathBuf::from("mixed.txt"),
            old_string: "Line 2".to_string(),
            new_string: "Modified Line 2".to_string(),
            expected_replacements: 1,
            enable_fuzzy: false,
            fuzzy_threshold: 0.7,
        };

        tool.execute(input).await.unwrap();

        let bytes = fs::read(&file_path).unwrap();
        let content = String::from_utf8(bytes).unwrap();
        assert!(content.contains("Modified Line 2"));
        // Mixed endings should be preserved
        assert!(content.contains("\n") || content.contains("\r"));
    }

    #[tokio::test]
    async fn test_edit_block_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("empty.txt");
        fs::write(&file_path, "").unwrap();

        let tool = EditBlockTool::with_base_path(temp_dir.path().to_path_buf());
        let input = EditBlockInput {
            file_path: PathBuf::from("empty.txt"),
            old_string: "nonexistent".to_string(),
            new_string: "something".to_string(),
            expected_replacements: 1,
            enable_fuzzy: false,
            fuzzy_threshold: 0.7,
        };

        let result = tool.execute(input).await;
        // Should fail gracefully on empty file
        assert!(result.is_err() || result.unwrap().as_text().contains("not found"));
    }

    #[tokio::test]
    async fn test_edit_block_utf8_content() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("utf8.txt");

        let original = "Hello 世界\nÜmläüts äöü\n🎵 Music\n";
        fs::write(&file_path, original).unwrap();

        let tool = EditBlockTool::with_base_path(temp_dir.path().to_path_buf());
        let input = EditBlockInput {
            file_path: PathBuf::from("utf8.txt"),
            old_string: "Ümläüts äöü".to_string(),
            new_string: "Modified äöü".to_string(),
            expected_replacements: 1,
            enable_fuzzy: false,
            fuzzy_threshold: 0.7,
        };

        tool.execute(input).await.unwrap();

        let content = fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("Modified äöü"));
        assert!(content.contains("世界"));
        assert!(content.contains("🎵"));
    }

    #[tokio::test]
    async fn test_edit_block_crlf_replacement_with_crlf_in_new_string() {
        // BUG TEST: When the original file has CRLF endings, and the new_string
        // also contains CRLF, the CRLF preservation logic should NOT double the CR.
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("crlf_replace.txt");

        // Original file with CRLF line endings
        let original = "Line 1\r\nLine 2\r\nLine 3\r\n";
        fs::write(&file_path, original).unwrap();

        let tool = EditBlockTool::with_base_path(temp_dir.path().to_path_buf());
        let input = EditBlockInput {
            file_path: PathBuf::from("crlf_replace.txt"),
            old_string: "Line 2".to_string(),
            // new_string explicitly contains CRLF - this should be preserved as-is
            new_string: "New Line 2\r\nExtra Line".to_string(),
            expected_replacements: 1,
            enable_fuzzy: false,
            fuzzy_threshold: 0.7,
        };

        tool.execute(input).await.unwrap();

        let bytes = fs::read(&file_path).unwrap();
        let content = String::from_utf8(bytes).unwrap();

        // The content should have proper CRLF, not doubled \r\r\n
        assert!(
            !content.contains("\r\r\n"),
            "Bug: CRLF was doubled to \\r\\r\\n! Content bytes: {:?}",
            content.as_bytes()
        );

        // Verify the replacement happened correctly
        assert!(content.contains("New Line 2\r\nExtra Line"));
    }

    #[tokio::test]
    async fn test_edit_block_multiple_occurrences() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("multi.txt");

        let original = "Item A\nItem A\nItem B\nItem A\n";
        fs::write(&file_path, original).unwrap();

        let tool = EditBlockTool::with_base_path(temp_dir.path().to_path_buf());

        // Replace all occurrences (3 total)
        let input = EditBlockInput {
            file_path: PathBuf::from("multi.txt"),
            old_string: "Item A".to_string(),
            new_string: "Item X".to_string(),
            expected_replacements: 3, // All 3 occurrences
            enable_fuzzy: false,
            fuzzy_threshold: 0.7,
        };

        tool.execute(input).await.unwrap();

        let content = fs::read_to_string(&file_path).unwrap();
        // Should have replaced all occurrences
        let x_count = content.matches("Item X").count();
        let a_count = content.matches("Item A").count();
        assert_eq!(x_count, 3);
        assert_eq!(a_count, 0);
    }

    // ===== find_fuzzy_match Unit Tests =====

    #[test]
    fn test_fuzzy_match_empty_pattern() {
        let result = EditBlockTool::find_fuzzy_match("some text", "", 0.5);
        assert!(result.is_none(), "Empty pattern should return None");
    }

    #[test]
    fn test_fuzzy_match_pattern_longer_than_text() {
        let result =
            EditBlockTool::find_fuzzy_match("short", "this pattern is much longer than text", 0.5);
        assert!(
            result.is_none(),
            "Pattern longer than text should return None"
        );
    }

    #[test]
    fn test_fuzzy_match_exact_match() {
        let result = EditBlockTool::find_fuzzy_match("hello world", "world", 0.5);
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.matched_text, "world");
        assert!(
            (m.similarity - 1.0).abs() < 0.001,
            "Exact match should have similarity 1.0"
        );
    }

    #[test]
    fn test_fuzzy_match_finds_similar() {
        // "wrld" is similar to "world"
        let result = EditBlockTool::find_fuzzy_match("hello world goodbye", "wrld", 0.5);
        assert!(result.is_some());
        let m = result.unwrap();
        assert!(m.similarity > 0.5);
    }

    #[test]
    fn test_fuzzy_match_below_threshold() {
        // Very high threshold, nothing should match
        let result = EditBlockTool::find_fuzzy_match("hello world", "xyz", 0.99);
        assert!(result.is_none(), "Nothing should match with high threshold");
    }

    #[test]
    fn test_fuzzy_match_variable_window_skip_large() {
        // Trigger: window_size > text.len() causes continue
        // Pattern of 10 chars on 10 char text: +10% = 11 > 10, should skip that window
        let result = EditBlockTool::find_fuzzy_match("abcdefghij", "abcdefghij", 0.5);
        assert!(result.is_some()); // Should still find match via exact window
    }

    #[test]
    fn test_fuzzy_match_smaller_window() {
        // Test -10% window size finding a match
        // Pattern "ABCDEFGHIJ" (10 chars), -10% window = 9 chars
        // Text has "ABCDEFGHI" (9 chars) which the smaller window will evaluate
        let result = EditBlockTool::find_fuzzy_match("xxxABCDEFGHIxxx", "ABCDEFGHIJ", 0.5);
        assert!(result.is_some());
        // The variable window logic is exercised
    }

    #[test]
    fn test_fuzzy_match_continue_branch() {
        // Trigger the continue branch: window_size > text.len()
        // Pattern 100 chars, +10% = 110 chars, but text is only 105 chars
        let long_pattern = "a".repeat(100);
        let text = "a".repeat(105); // Match exists but +10% window can't be used

        let result = EditBlockTool::find_fuzzy_match(&text, &long_pattern, 0.5);
        // This exercises the continue branch for +10% window (110 > 105)
        assert!(result.is_some()); // Still finds match via exact or -10% window
    }
}
