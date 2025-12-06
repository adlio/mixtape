//! Token estimation utilities
//!
//! Simple utilities for estimating token counts. The default implementations
//! in `Model` and `ModelProvider` use ~4 characters per token, but you can
//! use these utilities for custom token estimation.

/// Simple character-based token estimator
/// Uses ~4 characters per token heuristic (common approximation)
#[derive(Debug, Clone, Default)]
pub struct CharacterTokenizer {
    chars_per_token: usize,
}

impl CharacterTokenizer {
    /// Create a new tokenizer with the default 4 characters per token
    pub fn new() -> Self {
        Self { chars_per_token: 4 }
    }

    /// Create a tokenizer with a custom characters-per-token ratio
    pub fn with_chars_per_token(chars_per_token: usize) -> Self {
        Self { chars_per_token }
    }

    /// Estimate the number of tokens in the given text
    pub fn estimate_tokens(&self, text: &str) -> usize {
        text.len().div_ceil(self.chars_per_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_character_tokenizer() {
        let tokenizer = CharacterTokenizer::new();

        // ~4 chars per token (rounds up)
        assert_eq!(tokenizer.estimate_tokens("hell"), 1); // 4 chars = 1 token
        assert_eq!(tokenizer.estimate_tokens("hello"), 2); // 5 chars rounds up to 2 tokens
        assert_eq!(tokenizer.estimate_tokens("hello world"), 3); // 11 chars = 3 tokens
        assert_eq!(tokenizer.estimate_tokens("this is a longer sentence"), 7); // 26 chars = 7 tokens
    }
}
