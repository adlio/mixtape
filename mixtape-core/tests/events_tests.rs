use mixtape_core::TokenUsage;

#[test]
fn test_token_usage_total() {
    let usage = TokenUsage {
        input_tokens: 100,
        output_tokens: 50,
    };

    assert_eq!(usage.total(), 150);

    // Test with zeros
    let zero_usage = TokenUsage {
        input_tokens: 0,
        output_tokens: 0,
    };
    assert_eq!(zero_usage.total(), 0);

    // Test with large numbers
    let large_usage = TokenUsage {
        input_tokens: 1_000_000,
        output_tokens: 500_000,
    };
    assert_eq!(large_usage.total(), 1_500_000);
}
