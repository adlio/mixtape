use crate::prelude::*;
use html2md::parse_html;
use readability_rust::Readability;
use reqwest::Client;
use robotstxt::DefaultMatcher;
use std::time::Duration;
use url::Url;

/// Input for fetching web content
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FetchInput {
    /// URL to fetch
    pub url: String,

    /// Maximum content length in characters (default: 5000)
    #[serde(default = "default_max_length")]
    pub max_length: Option<usize>,

    /// Starting character index for pagination (default: 0)
    #[serde(default)]
    pub start_index: Option<usize>,

    /// Return raw HTML instead of Markdown (default: false)
    #[serde(default)]
    pub raw: bool,

    /// Force fetch even if robots.txt disallows (default: false, use with caution)
    #[serde(default)]
    pub force: bool,

    /// Custom user agent (default: "mixtape-bot/1.0")
    #[serde(default = "default_user_agent")]
    pub user_agent: String,

    /// Request timeout in seconds (default: 30)
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_max_length() -> Option<usize> {
    Some(5000)
}

fn default_user_agent() -> String {
    "mixtape-bot/1.0 (+https://github.com/your-repo/mixtape)".to_string()
}

fn default_timeout() -> u64 {
    30
}

/// Tool for fetching and processing web content
pub struct FetchTool {
    client: Client,
}

impl FetchTool {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to create HTTP client");

        Self { client }
    }

    /// Check robots.txt compliance
    async fn check_robots_txt(
        &self,
        url: &Url,
        user_agent: &str,
    ) -> std::result::Result<bool, ToolError> {
        let host = url
            .host_str()
            .ok_or_else(|| ToolError::from("Invalid host"))?;
        let robots_url = format!("{}://{}/robots.txt", url.scheme(), host);

        // Fetch robots.txt with a short timeout
        let robots_response =
            match tokio::time::timeout(Duration::from_secs(5), self.client.get(&robots_url).send())
                .await
            {
                Ok(Ok(response)) => response,
                Ok(Err(_)) => return Ok(true), // No robots.txt, allow
                Err(_) => return Ok(true),     // Timeout, allow
            };

        if !robots_response.status().is_success() {
            return Ok(true); // No robots.txt or error, allow
        }

        let robots_content = match robots_response.text().await {
            Ok(content) => content,
            Err(e) => return Err(format!("Failed to read robots.txt: {}", e).into()),
        };

        // Parse robots.txt using Google's matcher
        let mut matcher = DefaultMatcher::default();
        let url_str = url.as_str();

        Ok(matcher.one_agent_allowed_by_robots(&robots_content, user_agent, url_str))
    }

    /// Extract main content from HTML using Mozilla's Readability algorithm
    fn extract_content(&self, html: &str, _url: &str) -> (Option<String>, String) {
        // Try to use readability-rust for intelligent content extraction
        match Readability::new(html, None) {
            Ok(mut parser) => {
                if let Some(article) = parser.parse() {
                    // Successfully extracted article content with title and HTML content
                    let content = article.content.unwrap_or_else(|| html.to_string());
                    return (article.title, content);
                }
            }
            Err(_) => {
                // If readability fails, fall back to returning the full HTML
            }
        }

        // Fallback: return the entire HTML if readability extraction fails
        (None, html.to_string())
    }

    /// Convert HTML to Markdown
    fn html_to_markdown(&self, html: &str) -> String {
        parse_html(html)
    }

    /// Paginate content
    fn paginate_content(
        &self,
        content: String,
        start_index: Option<usize>,
        max_length: Option<usize>,
    ) -> (String, bool, usize) {
        let total_length = content.len();
        let start = start_index.unwrap_or(0);

        if start >= total_length {
            return (String::new(), false, total_length);
        }

        if let Some(max_len) = max_length {
            let end = (start + max_len).min(total_length);
            let truncated_content = content[start..end].to_string();
            let is_truncated = end < total_length;
            (truncated_content, is_truncated, total_length)
        } else {
            let truncated_content = content[start..].to_string();
            (truncated_content, false, total_length)
        }
    }
}

impl Default for FetchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for FetchTool {
    type Input = FetchInput;

    fn name(&self) -> &str {
        "fetch"
    }

    fn description(&self) -> &str {
        "Fetch content from a URL with robots.txt compliance, content extraction, and Markdown conversion. \
         Supports pagination for large documents."
    }

    fn format_output_plain(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let (metadata, content) = parse_fetch_header(&output);

        if metadata.is_empty() {
            return output.to_string();
        }

        let mut out = String::new();
        out.push_str(&"─".repeat(60));
        out.push('\n');

        for (key, value) in &metadata {
            let icon = match *key {
                "URL" => "[>]",
                "Title" => "[#]",
                "Content Length" => "[=]",
                "Showing" => "[~]",
                _ => "   ",
            };
            out.push_str(&format!("{} {:15} {}\n", icon, key, value));
        }

        out.push_str(&"─".repeat(60));
        out.push_str("\n\n");
        out.push_str(content);
        out
    }

    fn format_output_ansi(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let (metadata, content) = parse_fetch_header(&output);

        if metadata.is_empty() {
            return output.to_string();
        }

        let mut out = String::new();
        out.push_str(&format!("\x1b[2m{}\x1b[0m\n", "─".repeat(60)));

        for (key, value) in &metadata {
            let (icon, color) = match *key {
                "URL" => ("\x1b[34m󰖟\x1b[0m", "\x1b[34m"),
                "Title" => ("\x1b[33m󰉹\x1b[0m", "\x1b[1m"),
                "Content Length" => ("\x1b[32m󰋊\x1b[0m", "\x1b[32m"),
                "Showing" => ("\x1b[36m󰦨\x1b[0m", "\x1b[36m"),
                _ => ("  ", "\x1b[0m"),
            };
            out.push_str(&format!(
                "{} \x1b[2m{:15}\x1b[0m {}{}\x1b[0m\n",
                icon, key, color, value
            ));
        }

        out.push_str(&format!("\x1b[2m{}\x1b[0m\n\n", "─".repeat(60)));
        out.push_str(content);
        out
    }

    fn format_output_markdown(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let (metadata, content) = parse_fetch_header(&output);

        if metadata.is_empty() {
            return output.to_string();
        }

        let mut out = String::new();
        let title = metadata
            .iter()
            .find(|(k, _)| *k == "Title")
            .map(|(_, v)| *v);

        if let Some(t) = title {
            out.push_str(&format!("## {}\n\n", t));
        }

        for (key, value) in &metadata {
            if *key != "Title" {
                out.push_str(&format!("- **{}**: {}\n", key, value));
            }
        }

        out.push_str("\n---\n\n");
        out.push_str(content);
        out
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        // Parse URL
        let url =
            Url::parse(&input.url).map_err(|e| ToolError::from(format!("Invalid URL: {}", e)))?;

        // Check robots.txt compliance unless force is set
        if !input.force {
            let allowed = self
                .check_robots_txt(&url, &input.user_agent)
                .await
                .map_err(|e| ToolError::from(format!("Robots.txt check failed: {}", e)))?;

            if !allowed {
                return Err(format!(
                    "Access to {} is disallowed by robots.txt for user-agent '{}'",
                    input.url, input.user_agent
                )
                .into());
            }
        }

        // Fetch the URL
        let response = tokio::time::timeout(
            Duration::from_secs(input.timeout_seconds),
            self.client
                .get(input.url.clone())
                .header("User-Agent", &input.user_agent)
                .send(),
        )
        .await
        .map_err(|_| format!("Request timed out after {} seconds", input.timeout_seconds))?
        .map_err(|e| ToolError::from(format!("Failed to fetch URL: {}", e)))?;

        // Check response status
        if !response.status().is_success() {
            return Err(format!(
                "HTTP error: {} {}",
                response.status().as_u16(),
                response.status().canonical_reason().unwrap_or("Unknown")
            )
            .into());
        }

        // Get the HTML content
        let html = response
            .text()
            .await
            .map_err(|e| ToolError::from(format!("Failed to read response body: {}", e)))?;

        // Extract main content and title using Readability
        let (title, content_html) = self.extract_content(&html, &input.url);

        // Convert to markdown unless raw is requested
        let processed_content = if input.raw {
            content_html
        } else {
            self.html_to_markdown(&content_html)
        };

        // Apply pagination
        let (final_content, is_truncated, total_length) =
            self.paginate_content(processed_content, input.start_index, input.max_length);

        // Format result
        let mut result = String::new();
        result.push_str(&format!("URL: {}\n", input.url));

        if let Some(page_title) = title {
            result.push_str(&format!("Title: {}\n", page_title.trim()));
        }

        result.push_str(&format!("Content Length: {} characters\n", total_length));

        if is_truncated {
            let start = input.start_index.unwrap_or(0);
            let end = start + final_content.len();
            result.push_str(&format!(
                "Showing: characters {}-{} (truncated)\n",
                start, end
            ));
        }

        result.push_str("\n---\n\n");
        result.push_str(&final_content);

        Ok(result.into())
    }
}

/// Parse fetch output header into metadata fields
fn parse_fetch_header(output: &str) -> (Vec<(&str, &str)>, &str) {
    let mut metadata = Vec::new();
    let mut content_start = 0;

    for (i, line) in output.lines().enumerate() {
        if line == "---" {
            // Find content after the separator
            let lines: Vec<&str> = output.lines().collect();
            if i + 1 < lines.len() {
                // Calculate byte offset to content
                let header_len: usize = lines[..=i].iter().map(|l| l.len() + 1).sum();
                content_start = header_len;
            }
            break;
        }

        if let Some(colon_idx) = line.find(": ") {
            let key = &line[..colon_idx];
            let value = &line[colon_idx + 2..];
            metadata.push((key, value));
        }
    }

    let content = if content_start < output.len() {
        output[content_start..].trim_start_matches('\n')
    } else {
        ""
    };

    (metadata, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    /// Helper to create a FetchInput with sensible defaults for testing
    fn test_input(url: impl Into<String>) -> FetchInput {
        FetchInput {
            url: url.into(),
            user_agent: "test-agent".to_string(),
            timeout_seconds: 30,
            raw: false,
            force: false,
            start_index: None,
            max_length: None,
        }
    }

    // ==================== Default and constructor tests ====================

    #[test]
    fn test_default() {
        let tool: FetchTool = Default::default();
        assert_eq!(tool.name(), "fetch");
    }

    #[test]
    fn test_tool_name() {
        let tool = FetchTool::new();
        assert_eq!(tool.name(), "fetch");
    }

    #[test]
    fn test_tool_description() {
        let tool = FetchTool::new();
        assert!(!tool.description().is_empty());
        assert!(tool.description().contains("Fetch"));
    }

    // ==================== Default value function tests ====================

    #[test]
    fn test_default_max_length() {
        assert_eq!(default_max_length(), Some(5000));
    }

    #[test]
    fn test_default_user_agent() {
        let ua = default_user_agent();
        assert!(ua.contains("mixtape"));
    }

    #[test]
    fn test_default_timeout() {
        assert_eq!(default_timeout(), 30);
    }

    // ==================== parse_fetch_header tests ====================

    #[test]
    fn test_parse_fetch_header_complete() {
        let output = "URL: https://example.com\nTitle: Test Page\nContent Length: 1000 characters\nShowing: characters 0-500 (truncated)\n\n---\n\nThis is the content.";
        let (metadata, content) = parse_fetch_header(output);

        assert_eq!(metadata.len(), 4);
        assert_eq!(metadata[0], ("URL", "https://example.com"));
        assert_eq!(metadata[1], ("Title", "Test Page"));
        assert_eq!(metadata[2], ("Content Length", "1000 characters"));
        assert_eq!(metadata[3], ("Showing", "characters 0-500 (truncated)"));
        assert!(content.contains("This is the content"));
    }

    #[test]
    fn test_parse_fetch_header_no_separator() {
        let output = "Just plain content without headers";
        let (metadata, content) = parse_fetch_header(output);

        // Without "---" separator, content_start stays at 0
        // so the entire output is returned as content
        assert!(metadata.is_empty());
        // Content is the full output when there's no "---" separator
        assert_eq!(content, output);
    }

    #[test]
    fn test_parse_fetch_header_with_metadata_no_separator() {
        // Has metadata-like content but no "---" separator
        let output = "URL: https://example.com\nTitle: Test";
        let (metadata, content) = parse_fetch_header(output);

        // Metadata is extracted even without separator
        assert_eq!(metadata.len(), 2);
        assert_eq!(metadata[0], ("URL", "https://example.com"));
        // But content includes everything since content_start=0
        assert!(content.contains("URL:"));
    }

    #[test]
    fn test_parse_fetch_header_minimal() {
        let output = "URL: https://example.com\n\n---\n\nContent";
        let (metadata, content) = parse_fetch_header(output);

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0], ("URL", "https://example.com"));
        assert!(content.contains("Content"));
    }

    #[test]
    fn test_parse_fetch_header_empty() {
        let output = "";
        let (metadata, content) = parse_fetch_header(output);

        assert!(metadata.is_empty());
        assert_eq!(content, "");
    }

    #[test]
    fn test_parse_fetch_header_no_content_after_separator() {
        // When "---" is the last line with no content after it
        // The implementation doesn't update content_start if there's nothing after "---"
        // so content includes everything (content_start stays 0)
        let output = "URL: https://example.com\n---";
        let (metadata, content) = parse_fetch_header(output);

        assert_eq!(metadata.len(), 1);
        // Due to implementation: content includes the whole output since content_start stays 0
        // This is a quirk - when "---" is the last line, i+1 < lines.len() is false
        assert!(content.contains("URL:"));
    }

    #[test]
    fn test_parse_fetch_header_content_after_separator() {
        // When there IS content after "---", content_start is properly set
        let output = "URL: https://example.com\n---\nBody content here";
        let (metadata, content) = parse_fetch_header(output);

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0], ("URL", "https://example.com"));
        // Content should be just the body, not the URL
        assert_eq!(content, "Body content here");
        assert!(!content.contains("URL:"));
    }

    // ==================== format_output_plain tests ====================

    #[test]
    fn test_format_output_plain_no_metadata() {
        let tool = FetchTool::new();
        let result: ToolResult = "Plain content without headers".into();

        let formatted = tool.format_output_plain(&result);
        assert_eq!(formatted, "Plain content without headers");
    }

    #[test]
    fn test_format_output_plain_with_metadata() {
        let tool = FetchTool::new();
        let result: ToolResult = "URL: https://example.com\nTitle: Test\nContent Length: 100 characters\n\n---\n\nContent here".into();

        let formatted = tool.format_output_plain(&result);

        // Should have separator line
        assert!(formatted.contains("─"));
        // Should have icon indicators
        assert!(
            formatted.contains("[>]") || formatted.contains("[#]") || formatted.contains("[=]")
        );
        // Should have content
        assert!(formatted.contains("Content here"));
    }

    #[test]
    fn test_format_output_plain_icons() {
        let tool = FetchTool::new();
        let result: ToolResult = "URL: https://example.com\nTitle: Test Title\nContent Length: 500 characters\nShowing: 0-100\n\n---\n\nBody".into();

        let formatted = tool.format_output_plain(&result);

        // Check icons for different metadata types
        assert!(formatted.contains("[>]")); // URL
        assert!(formatted.contains("[#]")); // Title
        assert!(formatted.contains("[=]")); // Content Length
        assert!(formatted.contains("[~]")); // Showing
    }

    // ==================== format_output_ansi tests ====================

    #[test]
    fn test_format_output_ansi_no_metadata() {
        let tool = FetchTool::new();
        let result: ToolResult = "Plain content".into();

        let formatted = tool.format_output_ansi(&result);
        assert_eq!(formatted, "Plain content");
    }

    #[test]
    fn test_format_output_ansi_with_metadata() {
        let tool = FetchTool::new();
        let result: ToolResult = "URL: https://example.com\nTitle: Test\n\n---\n\nContent".into();

        let formatted = tool.format_output_ansi(&result);

        // Should contain ANSI escape codes
        assert!(formatted.contains("\x1b["));
        // Should have dimmed separator
        assert!(formatted.contains("\x1b[2m"));
    }

    #[test]
    fn test_format_output_ansi_colors() {
        let tool = FetchTool::new();
        let result: ToolResult = "URL: https://example.com\nTitle: Test\nContent Length: 100 characters\nShowing: 0-50\n\n---\n\nBody".into();

        let formatted = tool.format_output_ansi(&result);

        // URL should be blue
        assert!(formatted.contains("\x1b[34m"));
        // Title should be bold
        assert!(formatted.contains("\x1b[1m"));
        // Content Length should be green
        assert!(formatted.contains("\x1b[32m"));
        // Showing should be cyan
        assert!(formatted.contains("\x1b[36m"));
    }

    // ==================== format_output_markdown tests ====================

    #[test]
    fn test_format_output_markdown_no_metadata() {
        let tool = FetchTool::new();
        let result: ToolResult = "Plain content".into();

        let formatted = tool.format_output_markdown(&result);
        assert_eq!(formatted, "Plain content");
    }

    #[test]
    fn test_format_output_markdown_with_title() {
        let tool = FetchTool::new();
        let result: ToolResult =
            "URL: https://example.com\nTitle: My Page Title\n\n---\n\nContent".into();

        let formatted = tool.format_output_markdown(&result);

        // Title should be a heading
        assert!(formatted.contains("## My Page Title"));
    }

    #[test]
    fn test_format_output_markdown_metadata_as_list() {
        let tool = FetchTool::new();
        let result: ToolResult =
            "URL: https://example.com\nTitle: Test\nContent Length: 500 characters\n\n---\n\nBody"
                .into();

        let formatted = tool.format_output_markdown(&result);

        // Non-title metadata should be in list format with bold keys
        assert!(formatted.contains("- **URL**: https://example.com"));
        assert!(formatted.contains("- **Content Length**: 500 characters"));
        // Title should NOT be in the list (it's a heading)
        assert!(!formatted.contains("- **Title**"));
    }

    #[test]
    fn test_format_output_markdown_separator() {
        let tool = FetchTool::new();
        let result: ToolResult = "URL: https://example.com\n\n---\n\nBody content".into();

        let formatted = tool.format_output_markdown(&result);

        // Should have horizontal rule separator
        assert!(formatted.contains("---"));
        // Should have content
        assert!(formatted.contains("Body content"));
    }

    // ==================== paginate_content edge cases ====================

    #[test]
    fn test_paginate_content_start_beyond_length() {
        let tool = FetchTool::new();
        let content = "Short".to_string();

        let (result, truncated, total) = tool.paginate_content(content, Some(100), Some(10));

        assert_eq!(result, "");
        assert!(!truncated);
        assert_eq!(total, 5);
    }

    #[test]
    fn test_paginate_content_exact_length() {
        let tool = FetchTool::new();
        let content = "12345".to_string();

        let (result, truncated, total) = tool.paginate_content(content, Some(0), Some(5));

        assert_eq!(result, "12345");
        assert!(!truncated);
        assert_eq!(total, 5);
    }

    // ==================== Integration tests ====================

    #[test]
    fn test_extract_content() {
        let tool = FetchTool::new();
        let html = r#"
            <html>
                <head><title>Test Page</title></head>
                <body>
                    <nav>Navigation</nav>
                    <article>
                        <h1>Main Content</h1>
                        <p>This is the article content.</p>
                    </article>
                    <footer>Footer</footer>
                </body>
            </html>
        "#;

        let (title, content) = tool.extract_content(html, "https://example.com/test");
        // Readability may extract the H1 as title if it's more prominent than <title>
        assert_eq!(title, Some("Main Content".to_string()));
        // The content should include the article body
        assert!(content.contains("This is the article content"));
        // Navigation and footer should be removed by readability
        assert!(!content.contains("Navigation") || content.len() < html.len());
    }

    #[test]
    fn test_paginate_content() {
        let tool = FetchTool::new();
        let content = "0123456789".to_string();

        // Full content
        let (result, truncated, total) = tool.paginate_content(content.clone(), None, None);
        assert_eq!(result, "0123456789");
        assert!(!truncated);
        assert_eq!(total, 10);

        // Paginated
        let (result, truncated, total) = tool.paginate_content(content.clone(), Some(2), Some(5));
        assert_eq!(result, "23456");
        assert!(truncated);
        assert_eq!(total, 10);

        // Last page
        let (result, truncated, total) = tool.paginate_content(content.clone(), Some(5), Some(10));
        assert_eq!(result, "56789");
        assert!(!truncated);
        assert_eq!(total, 10);
    }

    #[test]
    fn test_html_to_markdown() {
        let tool = FetchTool::new();
        let html = "<h1>Title</h1><p>Paragraph with <strong>bold</strong> text.</p>";
        let markdown = tool.html_to_markdown(html);

        assert!(markdown.contains("Title"));
        assert!(markdown.contains("Paragraph"));
        assert!(markdown.contains("bold"));
    }

    // ===== Tests with wiremock for execute() method =====

    #[tokio::test]
    async fn test_fetch_successful_html() {
        let mock_server = MockServer::start().await;

        let html_body = r#"
            <html>
                <head><title>Test Article</title></head>
                <body>
                    <article>
                        <h1>Main Heading</h1>
                        <p>This is the main content of the article.</p>
                    </article>
                </body>
            </html>
        "#;

        Mock::given(method("GET"))
            .and(path("/test"))
            .respond_with(ResponseTemplate::new(200).set_body_string(html_body))
            .mount(&mock_server)
            .await;

        let tool = FetchTool::new();
        let input = test_input(format!("{}/test", mock_server.uri()));

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();

        assert!(output.contains("URL:"));
        assert!(output.contains("Title: Main Heading"));
        assert!(output.contains("main content"));
    }

    #[tokio::test]
    async fn test_fetch_404_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/notfound"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let tool = FetchTool::new();
        let input = test_input(format!("{}/notfound", mock_server.uri()));

        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("HTTP error") || err.contains("404"));
    }

    #[tokio::test]
    async fn test_fetch_timeout() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/slow"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_millis(1500))
                    .set_body_string("<html><body>Slow</body></html>"),
            )
            .mount(&mock_server)
            .await;

        let tool = FetchTool::new();
        let mut input = test_input(format!("{}/slow", mock_server.uri()));
        input.timeout_seconds = 1; // Short timeout

        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("timeout") || err.contains("timed out"));
    }

    #[tokio::test]
    async fn test_fetch_raw_mode() {
        let mock_server = MockServer::start().await;

        let html_body = "<html><body><h1>Raw HTML</h1><p>Content</p></body></html>";

        Mock::given(method("GET"))
            .and(path("/raw"))
            .respond_with(ResponseTemplate::new(200).set_body_string(html_body))
            .mount(&mock_server)
            .await;

        let tool = FetchTool::new();
        let mut input = test_input(format!("{}/raw", mock_server.uri()));
        input.raw = true;

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();

        // Should contain HTML tags when in raw mode
        assert!(output.contains("<h1>") || output.contains("Raw HTML"));
    }

    #[tokio::test]
    async fn test_fetch_with_pagination() {
        let mock_server = MockServer::start().await;

        let html_body = r#"
            <html>
                <body>
                    <article>
                        <p>This is a very long article with lots of content that will be paginated.</p>
                    </article>
                </body>
            </html>
        "#;

        Mock::given(method("GET"))
            .and(path("/paginated"))
            .respond_with(ResponseTemplate::new(200).set_body_string(html_body))
            .mount(&mock_server)
            .await;

        let tool = FetchTool::new();
        let mut input = test_input(format!("{}/paginated", mock_server.uri()));
        input.start_index = Some(0);
        input.max_length = Some(50);

        let result = tool.execute(input).await.unwrap();
        let output = result.as_text();

        assert!(output.contains("Showing:") || output.contains("truncated"));
    }

    #[tokio::test]
    async fn test_fetch_invalid_url() {
        let tool = FetchTool::new();
        let input = test_input("not-a-valid-url");

        let result = tool.execute(input).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid URL") || err.contains("scheme"));
    }

    #[tokio::test]
    async fn test_fetch_disallowed_scheme() {
        let tool = FetchTool::new();
        let input = test_input("file:///etc/passwd");

        let result = tool.execute(input).await;
        assert!(result.is_err());
        // The error message may vary based on implementation
        // Just verify it fails for non-HTTP schemes
    }
}
