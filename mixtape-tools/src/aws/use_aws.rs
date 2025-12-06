//! AWS service integration tool for dynamic API calls.
//!
//! This module provides a universal interface to AWS services, allowing agents to
//! invoke any AWS API operation dynamically using SigV4 signing.
//!
//! # Examples
//!
//! ## Basic Usage with GetCallerIdentity
//!
//! ```no_run
//! use mixtape_core::Tool;
//! use mixtape_tools::aws::UseAwsTool;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let tool = UseAwsTool::new().await?;
//!     let input = serde_json::from_value(serde_json::json!({
//!         "service_name": "sts",
//!         "operation_name": "GetCallerIdentity",
//!         "parameters": {},
//!         "region": "us-east-1",
//!         "label": "Get AWS caller identity"
//!     }))?;
//!     let result = tool.execute(input).await?;
//!     println!("{}", result.as_text());
//!     Ok(())
//! }
//! ```
//!
//! ## Using a Specific Profile
//!
//! ```no_run
//! use mixtape_tools::aws::UseAwsTool;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let tool = UseAwsTool::builder()
//!         .profile("my-aws-profile")
//!         .build()
//!         .await?;
//!     Ok(())
//! }
//! ```
//!
//! ## DynamoDB Query Example
//!
//! ```no_run
//! use mixtape_core::Tool;
//! use mixtape_tools::aws::UseAwsTool;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let tool = UseAwsTool::new().await?;
//!     let input = serde_json::from_value(serde_json::json!({
//!         "service_name": "dynamodb",
//!         "operation_name": "Scan",
//!         "parameters": {
//!             "TableName": "my-table",
//!             "Limit": 10
//!         },
//!         "region": "us-west-2",
//!         "label": "Scan DynamoDB table"
//!     }))?;
//!     let result = tool.execute(input).await?;
//!     Ok(())
//! }
//! ```

use crate::prelude::*;
use aws_config::BehaviorVersion;
use aws_credential_types::provider::ProvideCredentials;
use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
use aws_sigv4::sign::v4;
use aws_types::region::Region;
use http::header::{CONTENT_TYPE, HOST};
use http::{HeaderValue, Method};
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

// ============================================================================
// Public Types
// ============================================================================

/// Input parameters for the AWS service tool.
///
/// # Required Fields
///
/// - `service_name`: AWS service (e.g., "sts", "dynamodb", "lambda")
/// - `operation_name`: API operation in PascalCase (e.g., "GetCallerIdentity")
/// - `region`: AWS region (e.g., "us-east-1")
/// - `label`: Human-readable description for logging
///
/// # Optional Fields
///
/// - `parameters`: Operation parameters as a JSON object (default: `{}`)
/// - `profile_name`: AWS profile from ~/.aws/credentials
#[derive(Debug, Deserialize, JsonSchema)]
pub struct UseAwsInput {
    /// The AWS service name (e.g., "sts", "s3", "dynamodb", "lambda", "ec2").
    /// Use lowercase service names as they appear in AWS endpoint URLs.
    pub service_name: String,

    /// The API operation to perform (e.g., "GetCallerIdentity", "ListBuckets").
    /// Use PascalCase as they appear in AWS API documentation.
    pub operation_name: String,

    /// Parameters for the operation as a JSON object.
    /// These are passed as the request body for JSON-based APIs.
    #[serde(default = "default_parameters")]
    pub parameters: serde_json::Value,

    /// AWS region for the API call (e.g., "us-east-1", "us-west-2").
    pub region: String,

    /// Human-readable description of what this operation does.
    /// Used for logging and display purposes.
    #[serde(default)]
    pub label: Option<String>,

    /// Optional AWS profile name from ~/.aws/credentials.
    /// If not specified, uses default credential chain.
    #[serde(default)]
    pub profile_name: Option<String>,
}

fn default_parameters() -> serde_json::Value {
    serde_json::json!({})
}

/// Tool for making AWS API calls using SigV4 signing.
///
/// This tool provides a universal interface to AWS services, allowing agents to
/// invoke any AWS API operation dynamically. It supports:
///
/// - All AWS services accessible via JSON-based APIs
/// - Multiple credential sources (environment, profiles, IAM roles, SSO)
/// - SigV4 request signing for authentication
/// - Automatic region-specific endpoint resolution
/// - Extensible service target prefix configuration
///
/// # Construction
///
/// This tool requires async initialization due to AWS credential loading.
/// Use `UseAwsTool::new().await` or `UseAwsTool::builder()...build().await`.
///
/// **Note**: Unlike other tools in this crate, `UseAwsTool` does not implement
/// `Default` because it requires async credential loading. Attempting to use
/// an uninitialized tool will result in credential errors.
///
/// # Safety
///
/// Operations that match mutative prefixes (Create, Delete, Update, etc.) will
/// include a warning in the output. The calling application should implement
/// appropriate confirmation mechanisms.
pub struct UseAwsTool {
    client: Client,
    credentials_provider: Arc<dyn ProvideCredentials>,
    service_targets: HashMap<String, String>,
    #[allow(dead_code)] // Stored for potential future use (e.g., per-request timeout override)
    timeout: Duration,
}

/// Builder for creating `UseAwsTool` instances with custom configuration.
///
/// # Example
///
/// ```no_run
/// use mixtape_tools::aws::UseAwsTool;
/// use std::time::Duration;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let tool = UseAwsTool::builder()
///         .profile("my-profile")
///         .timeout(Duration::from_secs(120))
///         .with_service_target("custom-service", "CustomService_20240101")
///         .build()
///         .await?;
///     Ok(())
/// }
/// ```
#[derive(Default)]
pub struct UseAwsToolBuilder {
    profile: Option<String>,
    timeout: Option<Duration>,
    custom_service_targets: HashMap<String, String>,
    credentials_provider: Option<Arc<dyn ProvideCredentials>>,
}

// ============================================================================
// Builder Implementation
// ============================================================================

impl UseAwsToolBuilder {
    /// Set the AWS profile to use for credentials.
    pub fn profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    /// Set the HTTP request timeout (default: 60 seconds).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Add a custom service target prefix for the x-amz-target header.
    ///
    /// This is useful for services not in the default mapping or for
    /// using different API versions.
    pub fn with_service_target(
        mut self,
        service_name: impl Into<String>,
        target_prefix: impl Into<String>,
    ) -> Self {
        self.custom_service_targets
            .insert(service_name.into(), target_prefix.into());
        self
    }

    /// Inject a custom credentials provider (useful for testing).
    ///
    /// When set, skips the default AWS credential chain and uses
    /// the provided credentials directly.
    pub fn credentials_provider(mut self, provider: Arc<dyn ProvideCredentials>) -> Self {
        self.credentials_provider = Some(provider);
        self
    }

    /// Build the `UseAwsTool` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No AWS credentials are found (and no custom provider was set)
    /// - The HTTP client fails to initialize
    pub async fn build(self) -> Result<UseAwsTool, ToolError> {
        let timeout = self.timeout.unwrap_or(Duration::from_secs(60));

        // Get credentials provider
        let credentials_provider = if let Some(provider) = self.credentials_provider {
            provider
        } else {
            let mut config_loader =
                aws_config::defaults(BehaviorVersion::latest()).region(Region::new("us-east-1"));

            if let Some(profile_name) = &self.profile {
                config_loader = config_loader.profile_name(profile_name);
            }

            let config = config_loader.load().await;

            config
                .credentials_provider()
                .map(Arc::from)
                .ok_or_else(|| ToolError::from("No AWS credentials found. Ensure AWS credentials are configured via environment variables, ~/.aws/credentials, or IAM role."))?
        };

        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| ToolError::from(format!("Failed to create HTTP client: {}", e)))?;

        // Merge default service targets with custom ones
        let mut service_targets = default_service_targets();
        for (k, v) in self.custom_service_targets {
            service_targets.insert(k, v);
        }

        Ok(UseAwsTool {
            client,
            credentials_provider,
            service_targets,
            timeout,
        })
    }
}

// ============================================================================
// UseAwsTool Implementation
// ============================================================================

impl UseAwsTool {
    /// Create a new `UseAwsTool` with default configuration.
    ///
    /// This loads credentials from the default AWS credential chain:
    /// 1. Environment variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)
    /// 2. Shared credentials file (~/.aws/credentials)
    /// 3. IAM instance profile (on EC2)
    /// 4. Container credentials (in ECS/Fargate)
    /// 5. SSO credentials (if configured)
    ///
    /// # Errors
    ///
    /// Returns an error if no AWS credentials are found.
    pub async fn new() -> Result<Self, ToolError> {
        Self::builder().build().await
    }

    /// Create a builder for custom configuration.
    pub fn builder() -> UseAwsToolBuilder {
        UseAwsToolBuilder::default()
    }

    /// Get the service target prefix for a service.
    fn get_service_target(&self, service_name: &str) -> String {
        self.service_targets
            .get(service_name)
            .cloned()
            .unwrap_or_else(|| service_name.to_string())
    }
}

// ============================================================================
// Tool Trait Implementation
// ============================================================================

impl Tool for UseAwsTool {
    type Input = UseAwsInput;

    fn name(&self) -> &str {
        "use_aws"
    }

    fn description(&self) -> &str {
        "Make AWS API calls using service and operation names. \
         Supports all AWS services with JSON-based APIs. \
         Use PascalCase operation names (e.g., 'ListBuckets', 'GetCallerIdentity')."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        // Validate required fields with actionable error messages
        validate_input(&input)?;

        let label = input
            .label
            .as_deref()
            .unwrap_or_else(|| &input.operation_name);

        // Check for mutative operations
        let is_mutative = is_mutative_operation(&input.operation_name);

        // Build and send the request
        let request = self
            .build_signed_request(
                &input.service_name,
                &input.operation_name,
                &input.parameters,
                &input.region,
            )
            .await
            .map_err(|e| {
                ToolError::from(format!(
                    "Failed to build request for {}.{} in {}: {}",
                    input.service_name, input.operation_name, input.region, e
                ))
            })?;

        let response = self.client.execute(request).await.map_err(|e| {
            ToolError::from(format!(
                "AWS request failed for {}.{} in {}: {}",
                input.service_name, input.operation_name, input.region, e
            ))
        })?;

        let status = response.status();
        let body = response.text().await.map_err(|e| {
            ToolError::from(format!(
                "Failed to read response from {}.{}: {}",
                input.service_name, input.operation_name, e
            ))
        })?;

        if !status.is_success() {
            return Err(parse_aws_error(
                &input.service_name,
                &input.operation_name,
                &input.region,
                status,
                &body,
            ));
        }

        // Parse and format success response
        let response_json: serde_json::Value = serde_json::from_str(&body)
            .unwrap_or_else(|_| serde_json::json!({ "raw_response": body }));

        // Build result with metadata
        let mut result = String::with_capacity(body.len() + 256);

        result.push_str(&format!("Service: {}\n", input.service_name));
        result.push_str(&format!("Operation: {}\n", input.operation_name));
        result.push_str(&format!("Region: {}\n", input.region));
        result.push_str(&format!("Label: {}\n", label));

        if is_mutative {
            result.push_str("Warning: This was a mutative operation\n");
        }

        result.push_str("\n---\n\n");

        let pretty_response = serde_json::to_string_pretty(&response_json)
            .unwrap_or_else(|_| response_json.to_string());
        result.push_str(&pretty_response);

        Ok(ToolResult::text(result))
    }

    fn format_output_plain(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let (metadata, content) = parse_output_header(&output);

        if metadata.is_empty() {
            return output.to_string();
        }

        let mut out = String::new();
        out.push_str(&"─".repeat(60));
        out.push('\n');

        for (key, value) in &metadata {
            let icon = match *key {
                "Service" => "[S]",
                "Operation" => "[O]",
                "Region" => "[R]",
                "Label" => "[L]",
                "Warning" => "[!]",
                _ => "   ",
            };
            out.push_str(&format!("{} {:12} {}\n", icon, key, value));
        }

        out.push_str(&"─".repeat(60));
        out.push_str("\n\n");
        out.push_str(content);
        out
    }

    fn format_output_ansi(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let (metadata, content) = parse_output_header(&output);

        if metadata.is_empty() {
            return output.to_string();
        }

        let mut out = String::new();
        out.push_str(&format!("\x1b[2m{}\x1b[0m\n", "─".repeat(60)));

        for (key, value) in &metadata {
            let (icon, color) = match *key {
                "Service" => ("\x1b[33m\x1b[0m", "\x1b[33m"),
                "Operation" => ("\x1b[34m\x1b[0m", "\x1b[34m"),
                "Region" => ("\x1b[36m\x1b[0m", "\x1b[36m"),
                "Label" => ("\x1b[32m\x1b[0m", "\x1b[32m"),
                "Warning" => ("\x1b[31m\x1b[0m", "\x1b[31m"),
                _ => ("  ", "\x1b[0m"),
            };
            out.push_str(&format!(
                "{} \x1b[2m{:12}\x1b[0m {}{}\x1b[0m\n",
                icon, key, color, value
            ));
        }

        out.push_str(&format!("\x1b[2m{}\x1b[0m\n\n", "─".repeat(60)));
        out.push_str(content);
        out
    }

    fn format_output_markdown(&self, result: &ToolResult) -> String {
        let output = result.as_text();
        let (metadata, content) = parse_output_header(&output);

        if metadata.is_empty() {
            return output.to_string();
        }

        let mut out = String::new();

        let label = metadata
            .iter()
            .find(|(k, _)| *k == "Label")
            .map(|(_, v)| *v);

        if let Some(l) = label {
            out.push_str(&format!("## {}\n\n", l));
        }

        for (key, value) in &metadata {
            if *key != "Label" {
                out.push_str(&format!("- **{}**: {}\n", key, value));
            }
        }

        out.push_str("\n---\n\n");
        out.push_str("```json\n");
        out.push_str(content);
        out.push_str("\n```");
        out
    }
}

// ============================================================================
// Private Implementation Details
// ============================================================================

impl UseAwsTool {
    /// Build and sign an AWS API request.
    async fn build_signed_request(
        &self,
        service_name: &str,
        operation_name: &str,
        parameters: &serde_json::Value,
        region: &str,
    ) -> Result<reqwest::Request, ToolError> {
        let endpoint = get_endpoint(service_name, region);

        let credentials = self
            .credentials_provider
            .provide_credentials()
            .await
            .map_err(|e| ToolError::from(format!("Failed to get AWS credentials: {}", e)))?;

        let body = serde_json::to_string(parameters)
            .map_err(|e| ToolError::from(format!("Failed to serialize parameters: {}", e)))?;

        let content_type = "application/x-amz-json-1.1; charset=utf-8";
        let target_header = format!(
            "{}.{}",
            self.get_service_target(service_name),
            operation_name
        );

        let url = url::Url::parse(&endpoint)
            .map_err(|e| ToolError::from(format!("Invalid endpoint URL: {}", e)))?;
        let host = url
            .host_str()
            .ok_or_else(|| ToolError::from("Endpoint has no host"))?;

        let mut builder = http::Request::builder()
            .method(Method::POST)
            .uri(&endpoint)
            .header(HOST, host)
            .header(CONTENT_TYPE, HeaderValue::from_static(content_type))
            .header(
                "x-amz-target",
                HeaderValue::from_str(&target_header).unwrap(),
            );

        if let Some(token) = credentials.session_token() {
            builder = builder.header(
                "x-amz-security-token",
                HeaderValue::from_str(token).unwrap(),
            );
        }

        let http_request = builder
            .body(body.clone())
            .map_err(|e| ToolError::from(format!("Failed to build request: {}", e)))?;

        let signing_settings = SigningSettings::default();
        let identity = credentials.into();
        let signing_params = v4::SigningParams::builder()
            .identity(&identity)
            .region(region)
            .name(service_name)
            .time(SystemTime::now())
            .settings(signing_settings)
            .build()
            .map_err(|e| ToolError::from(format!("Failed to build signing params: {}", e)))?;

        let signable_request = SignableRequest::new(
            http_request.method().as_str(),
            http_request.uri().to_string(),
            http_request
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str(), v.to_str().unwrap_or(""))),
            SignableBody::Bytes(body.as_bytes()),
        )
        .map_err(|e| ToolError::from(format!("Failed to create signable request: {}", e)))?;

        let (signing_instructions, _signature) = sign(signable_request, &signing_params.into())
            .map_err(|e| ToolError::from(format!("Failed to sign request: {}", e)))?
            .into_parts();

        let mut req_builder = self.client.post(&endpoint).body(body);

        for (name, value) in http_request.headers() {
            if let Ok(v) = value.to_str() {
                req_builder = req_builder.header(name.as_str(), v);
            }
        }

        for (name, value) in signing_instructions.headers() {
            let name_str: &str = name;
            let value_str = std::str::from_utf8(value.as_bytes()).unwrap_or("");
            req_builder = req_builder.header(name_str, value_str);
        }

        req_builder
            .build()
            .map_err(|e| ToolError::from(format!("Failed to build final request: {}", e)))
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Validate input fields and provide actionable error messages.
fn validate_input(input: &UseAwsInput) -> Result<(), ToolError> {
    if input.service_name.is_empty() {
        return Err(ToolError::from(
            "service_name cannot be empty. Use lowercase AWS service names like 'sts', 'dynamodb', 's3'.",
        ));
    }
    if input.operation_name.is_empty() {
        return Err(ToolError::from(
            "operation_name cannot be empty. Use PascalCase operation names like 'GetCallerIdentity', 'ListBuckets'.",
        ));
    }
    if input.region.is_empty() {
        return Err(ToolError::from(
            "region cannot be empty. Use AWS region codes like 'us-east-1', 'eu-west-1'.",
        ));
    }

    // Validate parameters is an object (not array, string, etc.)
    if !input.parameters.is_object() {
        return Err(ToolError::from(format!(
            "parameters must be a JSON object, got: {}",
            match &input.parameters {
                serde_json::Value::Null => "null",
                serde_json::Value::Bool(_) => "boolean",
                serde_json::Value::Number(_) => "number",
                serde_json::Value::String(_) => "string",
                serde_json::Value::Array(_) => "array",
                serde_json::Value::Object(_) => "object",
            }
        )));
    }

    Ok(())
}

/// Parse AWS error response and create an actionable error message.
fn parse_aws_error(
    service_name: &str,
    operation_name: &str,
    region: &str,
    status: reqwest::StatusCode,
    body: &str,
) -> ToolError {
    if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(body) {
        let error_type = error_json
            .get("__type")
            .or_else(|| error_json.get("Error").and_then(|e| e.get("Code")))
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");
        let error_message = error_json
            .get("message")
            .or_else(|| error_json.get("Message"))
            .or_else(|| error_json.get("Error").and_then(|e| e.get("Message")))
            .and_then(|v| v.as_str())
            .unwrap_or(body);

        ToolError::from(format!(
            "AWS API error for {}.{} in {} (HTTP {}): {} - {}",
            service_name, operation_name, region, status, error_type, error_message
        ))
    } else {
        ToolError::from(format!(
            "AWS API error for {}.{} in {} (HTTP {}): {}",
            service_name, operation_name, region, status, body
        ))
    }
}

/// List of operation prefixes that indicate potentially mutative operations.
const MUTATIVE_OPERATIONS: &[&str] = &[
    "Create",
    "Put",
    "Delete",
    "Update",
    "Terminate",
    "Revoke",
    "Disable",
    "Deregister",
    "Stop",
    "Add",
    "Modify",
    "Remove",
    "Attach",
    "Detach",
    "Start",
    "Enable",
    "Register",
    "Set",
    "Associate",
    "Disassociate",
    "Allocate",
    "Release",
    "Cancel",
    "Reboot",
    "Accept",
];

/// Check if an operation is potentially mutative (destructive).
fn is_mutative_operation(operation_name: &str) -> bool {
    MUTATIVE_OPERATIONS
        .iter()
        .any(|prefix| operation_name.starts_with(prefix))
}

/// Get the AWS endpoint URL for a service and region.
fn get_endpoint(service_name: &str, region: &str) -> String {
    match service_name {
        "iam" => "https://iam.amazonaws.com".to_string(),
        "sts" if region == "us-east-1" => "https://sts.amazonaws.com".to_string(),
        "sts" => format!("https://sts.{}.amazonaws.com", region),
        "route53" | "cloudfront" => format!("https://{}.amazonaws.com", service_name),
        "s3" => format!("https://s3.{}.amazonaws.com", region),
        _ => format!("https://{}.{}.amazonaws.com", service_name, region),
    }
}

/// Default service target prefixes for the x-amz-target header.
fn default_service_targets() -> HashMap<String, String> {
    let mut targets = HashMap::new();
    targets.insert("dynamodb".into(), "DynamoDB_20120810".into());
    targets.insert("kinesis".into(), "Kinesis_20131202".into());
    targets.insert("logs".into(), "Logs_20140328".into());
    targets.insert("events".into(), "AWSEvents".into());
    targets.insert("lambda".into(), "AWSLambda".into());
    targets.insert("sts".into(), "AWSSecurityTokenServiceV20110615".into());
    targets.insert("sqs".into(), "AmazonSQS".into());
    targets.insert("sns".into(), "AmazonSimpleNotificationService".into());
    targets.insert("secretsmanager".into(), "secretsmanager".into());
    targets.insert("ssm".into(), "AmazonSSM".into());
    targets.insert("kms".into(), "TrentService".into());
    targets.insert("iam".into(), "IAMService".into());
    targets.insert(
        "cognito-idp".into(),
        "AWSCognitoIdentityProviderService".into(),
    );
    targets.insert(
        "cognito-identity".into(),
        "AWSCognitoIdentityService".into(),
    );
    targets.insert("cloudwatch".into(), "GraniteServiceVersion20100801".into());
    targets.insert(
        "application-autoscaling".into(),
        "AnyScaleFrontendService".into(),
    );
    targets.insert("elasticache".into(), "AmazonElastiCacheV9".into());
    targets.insert("ecr".into(), "AmazonEC2ContainerRegistry_V20150921".into());
    targets.insert("ecs".into(), "AmazonEC2ContainerServiceV20141113".into());
    targets.insert("cloudformation".into(), "CloudFormation".into());
    targets.insert("codepipeline".into(), "CodePipeline_20150709".into());
    targets.insert("codebuild".into(), "CodeBuild_20161006".into());
    targets.insert("codecommit".into(), "CodeCommit_20150413".into());
    targets.insert("codedeploy".into(), "CodeDeploy_20141006".into());
    targets.insert("stepfunctions".into(), "AWSStepFunctions".into());
    targets.insert("glue".into(), "AWSGlue".into());
    targets.insert("athena".into(), "AmazonAthena".into());
    targets.insert("redshift-data".into(), "RedshiftData".into());
    targets.insert("bedrock".into(), "AmazonBedrock".into());
    targets.insert("bedrock-runtime".into(), "AmazonBedrockRuntime".into());
    targets.insert("sagemaker".into(), "SageMaker".into());
    targets.insert("rekognition".into(), "RekognitionService".into());
    targets.insert("textract".into(), "Textract".into());
    targets.insert("comprehend".into(), "Comprehend_20171127".into());
    targets.insert(
        "translate".into(),
        "AWSShineFrontendService_20170701".into(),
    );
    targets.insert("polly".into(), "Parrot_v1".into());
    targets.insert("transcribe".into(), "Transcribe".into());
    targets
}

/// Parse output header into metadata fields and content.
/// This is used by formatting methods to separate metadata from response body.
fn parse_output_header(output: &str) -> (Vec<(&str, &str)>, &str) {
    let mut metadata = Vec::new();
    let mut content_start = 0;

    for (i, line) in output.lines().enumerate() {
        if line == "---" {
            let lines: Vec<&str> = output.lines().collect();
            if i + 1 < lines.len() {
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Builder tests ====================

    #[test]
    fn test_builder_default() {
        let builder = UseAwsToolBuilder::default();
        assert!(builder.profile.is_none());
        assert!(builder.timeout.is_none());
        assert!(builder.custom_service_targets.is_empty());
    }

    #[test]
    fn test_builder_profile() {
        let builder = UseAwsTool::builder().profile("my-profile");
        assert_eq!(builder.profile, Some("my-profile".to_string()));
    }

    #[test]
    fn test_builder_timeout() {
        let builder = UseAwsTool::builder().timeout(Duration::from_secs(120));
        assert_eq!(builder.timeout, Some(Duration::from_secs(120)));
    }

    #[test]
    fn test_builder_custom_service_target() {
        let builder = UseAwsTool::builder().with_service_target("custom", "CustomService_20240101");
        assert_eq!(
            builder.custom_service_targets.get("custom"),
            Some(&"CustomService_20240101".to_string())
        );
    }

    // ==================== Validation tests ====================

    #[test]
    fn test_validate_input_empty_service() {
        let input = UseAwsInput {
            service_name: String::new(),
            operation_name: "GetCallerIdentity".to_string(),
            parameters: serde_json::json!({}),
            region: "us-east-1".to_string(),
            label: None,
            profile_name: None,
        };
        let result = validate_input(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("service_name"));
    }

    #[test]
    fn test_validate_input_empty_operation() {
        let input = UseAwsInput {
            service_name: "sts".to_string(),
            operation_name: String::new(),
            parameters: serde_json::json!({}),
            region: "us-east-1".to_string(),
            label: None,
            profile_name: None,
        };
        let result = validate_input(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("operation_name"));
    }

    #[test]
    fn test_validate_input_empty_region() {
        let input = UseAwsInput {
            service_name: "sts".to_string(),
            operation_name: "GetCallerIdentity".to_string(),
            parameters: serde_json::json!({}),
            region: String::new(),
            label: None,
            profile_name: None,
        };
        let result = validate_input(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("region"));
    }

    #[test]
    fn test_validate_input_parameters_not_object() {
        let input = UseAwsInput {
            service_name: "sts".to_string(),
            operation_name: "GetCallerIdentity".to_string(),
            parameters: serde_json::json!([1, 2, 3]),
            region: "us-east-1".to_string(),
            label: None,
            profile_name: None,
        };
        let result = validate_input(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("array"));
    }

    #[test]
    fn test_validate_input_success() {
        let input = UseAwsInput {
            service_name: "sts".to_string(),
            operation_name: "GetCallerIdentity".to_string(),
            parameters: serde_json::json!({}),
            region: "us-east-1".to_string(),
            label: None,
            profile_name: None,
        };
        assert!(validate_input(&input).is_ok());
    }

    // ==================== Mutative operation detection ====================

    #[test]
    fn test_is_mutative_operation_create() {
        assert!(is_mutative_operation("CreateBucket"));
        assert!(is_mutative_operation("CreateTable"));
    }

    #[test]
    fn test_is_mutative_operation_delete() {
        assert!(is_mutative_operation("DeleteBucket"));
        assert!(is_mutative_operation("DeleteItem"));
    }

    #[test]
    fn test_is_mutative_operation_update() {
        assert!(is_mutative_operation("UpdateItem"));
        assert!(is_mutative_operation("UpdateTable"));
    }

    #[test]
    fn test_is_mutative_operation_put() {
        assert!(is_mutative_operation("PutObject"));
        assert!(is_mutative_operation("PutItem"));
    }

    #[test]
    fn test_is_mutative_operation_terminate() {
        assert!(is_mutative_operation("TerminateInstances"));
    }

    #[test]
    fn test_is_mutative_operation_non_mutative() {
        assert!(!is_mutative_operation("GetCallerIdentity"));
        assert!(!is_mutative_operation("ListBuckets"));
        assert!(!is_mutative_operation("DescribeInstances"));
        assert!(!is_mutative_operation("Scan"));
        assert!(!is_mutative_operation("Query"));
    }

    // ==================== Endpoint generation ====================

    #[test]
    fn test_get_endpoint_standard_service() {
        let endpoint = get_endpoint("dynamodb", "us-east-1");
        assert_eq!(endpoint, "https://dynamodb.us-east-1.amazonaws.com");
    }

    #[test]
    fn test_get_endpoint_sts_us_east_1() {
        let endpoint = get_endpoint("sts", "us-east-1");
        assert_eq!(endpoint, "https://sts.amazonaws.com");
    }

    #[test]
    fn test_get_endpoint_sts_other_region() {
        let endpoint = get_endpoint("sts", "us-west-2");
        assert_eq!(endpoint, "https://sts.us-west-2.amazonaws.com");
    }

    #[test]
    fn test_get_endpoint_iam() {
        let endpoint = get_endpoint("iam", "us-east-1");
        assert_eq!(endpoint, "https://iam.amazonaws.com");
    }

    #[test]
    fn test_get_endpoint_s3() {
        let endpoint = get_endpoint("s3", "us-west-2");
        assert_eq!(endpoint, "https://s3.us-west-2.amazonaws.com");
    }

    // ==================== Service target prefix ====================

    #[test]
    fn test_default_service_targets_contains_dynamodb() {
        let targets = default_service_targets();
        assert_eq!(
            targets.get("dynamodb"),
            Some(&"DynamoDB_20120810".to_string())
        );
    }

    #[test]
    fn test_default_service_targets_contains_sts() {
        let targets = default_service_targets();
        assert_eq!(
            targets.get("sts"),
            Some(&"AWSSecurityTokenServiceV20110615".to_string())
        );
    }

    #[test]
    fn test_default_service_targets_contains_lambda() {
        let targets = default_service_targets();
        assert_eq!(targets.get("lambda"), Some(&"AWSLambda".to_string()));
    }

    // ==================== Header parsing ====================

    #[test]
    fn test_parse_output_header_complete() {
        let output = "Service: sts\nOperation: GetCallerIdentity\nRegion: us-east-1\nLabel: Get identity\n\n---\n\n{\"Account\": \"123456789\"}";
        let (metadata, content) = parse_output_header(output);

        assert_eq!(metadata.len(), 4);
        assert_eq!(metadata[0], ("Service", "sts"));
        assert_eq!(metadata[1], ("Operation", "GetCallerIdentity"));
        assert_eq!(metadata[2], ("Region", "us-east-1"));
        assert_eq!(metadata[3], ("Label", "Get identity"));
        assert!(content.contains("Account"));
    }

    #[test]
    fn test_parse_output_header_no_separator() {
        let output = "Just plain content";
        let (metadata, content) = parse_output_header(output);

        assert!(metadata.is_empty());
        assert_eq!(content, output);
    }

    #[test]
    fn test_parse_output_header_with_warning() {
        let output = "Service: s3\nOperation: DeleteBucket\nWarning: This was a mutative operation\n\n---\n\n{}";
        let (metadata, _content) = parse_output_header(output);

        assert_eq!(metadata.len(), 3);
        assert_eq!(metadata[2], ("Warning", "This was a mutative operation"));
    }

    // ==================== Error parsing ====================

    #[test]
    fn test_parse_aws_error_with_type() {
        let body = r#"{"__type": "ValidationException", "message": "Invalid input"}"#;
        let error = parse_aws_error(
            "dynamodb",
            "PutItem",
            "us-east-1",
            reqwest::StatusCode::BAD_REQUEST,
            body,
        );
        let msg = error.to_string();

        assert!(msg.contains("dynamodb.PutItem"));
        assert!(msg.contains("us-east-1"));
        assert!(msg.contains("ValidationException"));
        assert!(msg.contains("Invalid input"));
    }

    #[test]
    fn test_parse_aws_error_with_nested_error() {
        let body = r#"{"Error": {"Code": "AccessDenied", "Message": "Access denied"}}"#;
        let error = parse_aws_error(
            "s3",
            "GetObject",
            "us-west-2",
            reqwest::StatusCode::FORBIDDEN,
            body,
        );
        let msg = error.to_string();

        assert!(msg.contains("s3.GetObject"));
        assert!(msg.contains("AccessDenied"));
        assert!(msg.contains("Access denied"));
    }

    #[test]
    fn test_parse_aws_error_plain_text() {
        let body = "Service unavailable";
        let error = parse_aws_error(
            "sts",
            "GetCallerIdentity",
            "us-east-1",
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            body,
        );
        let msg = error.to_string();

        assert!(msg.contains("sts.GetCallerIdentity"));
        assert!(msg.contains("us-east-1"));
        assert!(msg.contains("Service unavailable"));
    }
}
