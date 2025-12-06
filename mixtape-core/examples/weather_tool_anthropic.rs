// Example demonstrating real tool authoring with external API calls using Anthropic's direct API
//
// This shows how to build a production-quality tool that:
// - Calls an external API (National Weather Service - free, no auth required)
// - Handles errors properly (network, API, parsing)
// - Returns structured data the model can use
// - Has clear descriptions that help the model use it correctly
//
// Prerequisites: Set ANTHROPIC_API_KEY environment variable
//
// Run with: cargo run --example weather_tool_anthropic --features anthropic

use mixtape_core::{Agent, ClaudeHaiku4_5, Tool, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Input for the weather forecast tool
///
/// Doc comments on fields become "description" in the JSON schema,
/// helping the model understand how to use each parameter.
#[derive(Debug, Deserialize, JsonSchema)]
struct WeatherInput {
    /// Latitude of the location (e.g., 38.8894 for Washington DC)
    latitude: f64,
    /// Longitude of the location (e.g., -77.0352 for Washington DC)
    longitude: f64,
}

/// A period in the weather forecast
#[derive(Debug, Serialize, Deserialize)]
struct ForecastPeriod {
    name: String,
    temperature: i32,
    #[serde(rename = "temperatureUnit")]
    temperature_unit: String,
    #[serde(rename = "shortForecast")]
    short_forecast: String,
    #[serde(rename = "detailedForecast")]
    detailed_forecast: String,
}

/// NWS API response structures
#[derive(Debug, Deserialize)]
struct PointsResponse {
    properties: PointsProperties,
}

#[derive(Debug, Deserialize)]
struct PointsProperties {
    forecast: String,
}

#[derive(Debug, Deserialize)]
struct ForecastResponse {
    properties: ForecastProperties,
}

#[derive(Debug, Deserialize)]
struct ForecastProperties {
    periods: Vec<ForecastPeriod>,
}

/// Weather forecast tool using the National Weather Service API
struct WeatherTool {
    client: reqwest::Client,
}

impl WeatherTool {
    fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("mixtape-weather-example/1.0")
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Get the forecast URL for a location
    async fn get_forecast_url(&self, lat: f64, lon: f64) -> Result<String, ToolError> {
        let url = format!("https://api.weather.gov/points/{:.4},{:.4}", lat, lon);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ToolError::from(format!("Network error: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            // Provide helpful error messages for common issues
            if status.as_u16() == 404 {
                return Err(ToolError::from(
                    "Location not found. The NWS API only covers US locations. \
                     Make sure the coordinates are within the United States.",
                ));
            }

            return Err(ToolError::from(format!("API error ({}): {}", status, body)));
        }

        let points: PointsResponse = response
            .json()
            .await
            .map_err(|e| ToolError::from(format!("Failed to parse API response: {}", e)))?;

        Ok(points.properties.forecast)
    }

    /// Fetch the actual forecast
    async fn get_forecast(&self, forecast_url: &str) -> Result<Vec<ForecastPeriod>, ToolError> {
        let response = self
            .client
            .get(forecast_url)
            .send()
            .await
            .map_err(|e| ToolError::from(format!("Network error fetching forecast: {}", e)))?;

        if !response.status().is_success() {
            return Err(ToolError::from(format!(
                "Forecast API error: {}",
                response.status()
            )));
        }

        let forecast: ForecastResponse = response
            .json()
            .await
            .map_err(|e| ToolError::from(format!("Failed to parse forecast: {}", e)))?;

        Ok(forecast.properties.periods)
    }
}

impl Tool for WeatherTool {
    type Input = WeatherInput;

    fn name(&self) -> &str {
        "get_weather_forecast"
    }

    fn description(&self) -> &str {
        "Get the weather forecast for a US location using latitude and longitude. \
         Returns the forecast for the next several days including temperature, \
         conditions, and detailed descriptions. Only works for locations in the \
         United States (uses the National Weather Service API)."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        // Validate coordinates are roughly within the US
        if input.latitude < 24.0 || input.latitude > 50.0 {
            return Err(ToolError::from(
                "Latitude must be between 24 and 50 (continental US range)",
            ));
        }
        if input.longitude < -125.0 || input.longitude > -66.0 {
            return Err(ToolError::from(
                "Longitude must be between -125 and -66 (continental US range)",
            ));
        }

        // Get the forecast URL for this location
        let forecast_url = self
            .get_forecast_url(input.latitude, input.longitude)
            .await?;

        // Fetch the forecast
        let periods = self.get_forecast(&forecast_url).await?;

        // Format a readable summary (first 4 periods = ~2 days)
        let mut summary = String::new();
        for period in periods.iter().take(4) {
            summary.push_str(&format!(
                "**{}**: {}°{} - {}\n",
                period.name, period.temperature, period.temperature_unit, period.short_forecast
            ));
        }

        Ok(ToolResult::text(summary))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Weather Tool Example (Anthropic) ===\n");
    println!("This example shows a real tool that calls the NWS API.\n");

    let agent = Agent::builder()
        .anthropic_from_env(ClaudeHaiku4_5)
        .with_system_prompt(
            "You are a helpful weather assistant. Use the weather tool to answer \
             questions about US weather. For non-US locations, explain that the \
             tool only works for US locations.",
        )
        .add_tool(WeatherTool::new())
        .build()
        .await?;

    // Test with a simple question
    let question = "What's the weather forecast for Portland, OR ? \
                   (Hint: Portland is 45.5152° N, 122.6784° W)";

    println!("Question: {}\n", question);
    println!("---\n");

    let response = agent.run(question).await?;

    println!("{}\n", response.text);

    // Show execution stats
    println!("---");
    println!("Stats:");
    println!("  Duration: {:.2}s", response.duration.as_secs_f64());
    println!("  Model calls: {}", response.model_calls);
    println!("  Tool calls: {}", response.tool_calls.len());

    for tc in &response.tool_calls {
        println!(
            "    - {} ({:.2}s) {}",
            tc.name,
            tc.duration.as_secs_f64(),
            if tc.success { "✓" } else { "✗" }
        );
    }

    if let Some(usage) = &response.token_usage {
        println!(
            "  Tokens: {} input, {} output, {} total",
            usage.input_tokens,
            usage.output_tokens,
            usage.total()
        );
    }

    Ok(())
}
