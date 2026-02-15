//! Tests for the router builder.
//!
//! These tests verify the builder pattern and path configuration.
//! Note: Most tests require an actual agent instance which needs async initialization.
//! The tests below focus on testing the types and basic construction patterns.

use crate::router::MixtapeRouter;
use std::sync::Arc;

// Note: Full integration tests with real agents would require:
// 1. Async test infrastructure
// 2. Provider credentials/mocking
// 3. Complex setup
//
// These tests focus on the builder API surface and type safety.

#[test]
fn test_router_builder_type_signature() {
    // Verify that the builder accepts the correct types
    // This is a compile-time test
    fn _accepts_agent_owned(_: impl FnOnce(mixtape_core::Agent) -> MixtapeRouter) {}
    fn _accepts_agent_arc(_: impl FnOnce(Arc<mixtape_core::Agent>) -> MixtapeRouter) {}

    _accepts_agent_owned(MixtapeRouter::new);
    _accepts_agent_arc(MixtapeRouter::from_arc);
}

#[cfg(feature = "agui")]
#[test]
fn test_router_builder_fluent_api() {
    use crate::error::BuildError;

    // Test that the builder methods return Self for chaining
    // This is a compile-time test of the builder pattern

    // Verify the builder pattern compiles with method chaining
    fn _test_chaining<F>(f: F)
    where
        F: FnOnce(MixtapeRouter) -> Result<axum::Router, BuildError>,
    {
        drop(f);
    }

    #[cfg(feature = "agui")]
    _test_chaining(|builder| {
        builder
            .with_agui("/api/stream")
            .interrupt_path("/api/interrupt")
            .build()
    });

    _test_chaining(|builder| builder.with_agui("/api").build());
}

#[cfg(feature = "agui")]
#[test]
fn test_router_into_variants() {
    use crate::error::BuildError;

    // Test that both `build()` and `build_nested()` return Result<Router, BuildError>
    fn _returns_result(_: impl FnOnce(MixtapeRouter) -> Result<axum::Router, BuildError>) {}

    _returns_result(|b| b.with_agui("/api").build());
    _returns_result(|b| b.with_agui("/api").build_nested("/prefix"));
}

#[cfg(feature = "agui")]
#[test]
fn test_router_path_types() {
    // Test that path methods accept Into<String>
    fn _test_with_agui<S: Into<String>>(path: S) {
        // This would be: MixtapeRouter::new(agent).with_agui(path)
        // We're just testing the type signature
        drop(path.into());
    }

    _test_with_agui("/api/stream");
    _test_with_agui(String::from("/api/stream"));
    _test_with_agui("api/stream"); // No leading slash
}

#[cfg(feature = "agui")]
#[test]
fn test_router_builder_consumes_self() {
    // Test move semantics - compile-time verification
    // If this compiles, the builder correctly consumes self
    fn _consume_builder<F>(f: F)
    where
        F: FnOnce(MixtapeRouter),
    {
        drop(f);
    }

    _consume_builder(|router| {
        let _app = router.with_agui("/api").build();
        // router is moved here and can't be used again
    });
}

#[test]
fn test_app_state_construction() {
    // Test that AppState can be constructed from Arc<Agent>
    // This is used internally by the router
    use crate::state::AppState;

    fn _from_arc(agent: Arc<mixtape_core::Agent>) -> AppState {
        AppState::from_arc(agent)
    }

    let _ = _from_arc;
}

// Note: The following tests would require actual Agent instances:
// - test_router_new_wraps_agent_in_arc
// - test_router_from_arc
// - test_router_build_empty
// - test_router_with_agui_*
// - test_router_build_nested_*
//
// These would be better suited for integration tests with proper async setup.
