//! Shared transport for the two Bedrock Runtime OpenAI-compatible APIs.
//! No vendor endpoints, API-key storage, redirects, or stream reconnection.

use super::{invalid, protocol};
use crate::provider::ProviderError;
use aws_credential_types::{
    provider::{ProvideCredentials, SharedCredentialsProvider},
    Credentials,
};
use aws_sigv4::http_request::{sign, SignableBody, SignableRequest, SigningSettings};
use aws_sigv4::sign::v4;
use futures::{stream::BoxStream, StreamExt};
use reqwest::{
    header::{HeaderName, HeaderValue},
    Client, Request,
};
use std::time::{Duration, SystemTime};

pub(in crate::provider::bedrock) const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Closed set: callers cannot insert a URL, host, or arbitrary request path.
#[derive(Clone, Copy)]
pub(in crate::provider::bedrock) enum OpenAiApi {
    ChatCompletions,
    Responses,
}

impl OpenAiApi {
    fn path(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat/completions",
            Self::Responses => "responses",
        }
    }
}

pub(in crate::provider::bedrock) struct HttpResponse {
    pub status: u16,
    pub request_id: Option<String>,
    pub content_type: Option<String>,
    pub body: BoxStream<'static, Result<Vec<u8>, ProviderError>>,
}

#[async_trait::async_trait]
pub(in crate::provider::bedrock) trait RuntimeClient:
    Send + Sync
{
    fn region(&self) -> &str;
    async fn send(&self, body: Vec<u8>, streaming: bool) -> Result<HttpResponse, ProviderError>;
}

pub(in crate::provider::bedrock) struct SignedClient {
    http: Client,
    region: String,
    credentials: SharedCredentialsProvider,
    api: OpenAiApi,
}

impl SignedClient {
    pub fn from_config(
        config: &aws_config::SdkConfig,
        api: OpenAiApi,
    ) -> Result<Self, ProviderError> {
        if config.endpoint_url().is_some() {
            return Err(invalid(
                "Custom endpoints are not supported by the Bedrock OpenAI-compatible adapters",
            ));
        }
        let region = config
            .region()
            .ok_or_else(|| invalid("An explicit AWS Region is required"))?
            .as_ref()
            .to_owned();
        endpoint(&region, api)?;
        let credentials = config
            .credentials_provider()
            .ok_or_else(|| invalid("An AWS credentials provider is required"))?;
        let http = Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|_| invalid("Unable to construct the Bedrock HTTP client"))?;
        Ok(Self {
            http,
            region,
            credentials,
            api,
        })
    }
}

/// Other partitions require a documented endpoint contract, not a guessed suffix.
fn endpoint(region: &str, api: OpenAiApi) -> Result<String, ProviderError> {
    let parts: Vec<_> = region.split('-').collect();
    if parts.len() != 3
        || !matches!(
            parts[0],
            "us" | "eu" | "ap" | "sa" | "ca" | "me" | "af" | "il" | "mx"
        )
        || parts[1].is_empty()
        || !parts[1].bytes().all(|byte| byte.is_ascii_lowercase())
        || parts[2].is_empty()
        || !parts[2].bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(invalid(
            "A commercial AWS Region name is required for this Bedrock adapter",
        ));
    }
    Ok(format!(
        "https://bedrock-runtime.{region}.amazonaws.com/openai/v1/{}",
        api.path()
    ))
}

fn signed_request(
    client: &Client,
    region: &str,
    api: OpenAiApi,
    credentials: Credentials,
    body: Vec<u8>,
    streaming: bool,
    now: SystemTime,
) -> Result<Request, ProviderError> {
    let url = endpoint(region, api)?;
    let accept = if streaming {
        "text/event-stream"
    } else {
        "application/json"
    };
    let headers = [("content-type", "application/json"), ("accept", accept)];
    let identity = credentials.into();
    let params = v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name("bedrock")
        .time(now)
        .settings(SigningSettings::default())
        .build()
        .map_err(|_| invalid("Unable to configure Bedrock SigV4 signing"))?
        .into();
    let signable = SignableRequest::new(
        "POST",
        &url,
        headers.iter().copied(),
        SignableBody::Bytes(&body),
    )
    .map_err(|_| invalid("Unable to construct the Bedrock signing request"))?;
    let (instructions, _) = sign(signable, &params)
        .map_err(|_| ProviderError::Authentication("Bedrock request signing failed".into()))?
        .into_parts();
    let mut request = client
        .post(url)
        .header("content-type", "application/json")
        .header("accept", accept)
        .body(body)
        .build()
        .map_err(|_| invalid("Unable to construct the Bedrock HTTP request"))?;
    let (headers, query) = instructions.into_parts();
    if !query.is_empty() {
        return Err(invalid("Bedrock requires header-based request signing"));
    }
    for header in headers {
        let name = HeaderName::from_bytes(header.name().as_bytes())
            .map_err(|_| invalid("Invalid SigV4 header name"))?;
        let mut value = HeaderValue::from_str(header.value())
            .map_err(|_| invalid("Invalid SigV4 header value"))?;
        value.set_sensitive(
            header.sensitive() || matches!(name.as_str(), "authorization" | "x-amz-security-token"),
        );
        request.headers_mut().insert(name, value);
    }
    Ok(request)
}

#[async_trait::async_trait]
impl RuntimeClient for SignedClient {
    fn region(&self) -> &str {
        &self.region
    }

    async fn send(&self, body: Vec<u8>, streaming: bool) -> Result<HttpResponse, ProviderError> {
        // Refresh between calls and after retry delays, never freeze credentials
        // at construction or print credential-provider errors.
        let credentials = self.credentials.provide_credentials().await.map_err(|_| {
            ProviderError::Authentication("Unable to resolve AWS credentials for Bedrock".into())
        })?;
        let request = signed_request(
            &self.http,
            &self.region,
            self.api,
            credentials,
            body,
            streaming,
            SystemTime::now(),
        )?;
        let response = self.http.execute(request).await.map_err(|_| {
            ProviderError::Network("Bedrock HTTP request failed or timed out".into())
        })?;
        let request_id = ["x-amzn-requestid", "x-amzn-request-id", "x-request-id"]
            .iter()
            .find_map(|name| {
                response
                    .headers()
                    .get(*name)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned)
            });
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let status = response.status().as_u16();
        let body = response.bytes_stream().map(|chunk| {
            chunk
                .map(|bytes| bytes.to_vec())
                .map_err(|_| ProviderError::Network("Bedrock response body was interrupted".into()))
        });
        Ok(HttpResponse {
            status,
            request_id,
            content_type,
            body: Box::pin(body),
        })
    }
}

pub(in crate::provider::bedrock) fn status_error(status: u16) -> Option<ProviderError> {
    let message = format!("Bedrock Runtime returned HTTP {status}");
    match status {
        200 => None,
        401 | 403 => Some(ProviderError::Authentication(message)),
        429 => Some(ProviderError::RateLimited(message)),
        408 | 504 => Some(ProviderError::Network(message)),
        500 | 502 | 503 => Some(ProviderError::ServiceUnavailable(message)),
        400 | 404 | 422 => Some(ProviderError::Configuration(message)),
        _ => Some(ProviderError::Model(message)),
    }
}

pub(in crate::provider::bedrock) fn bounded_body(
    mut body: BoxStream<'static, Result<Vec<u8>, ProviderError>>,
) -> BoxStream<'static, Result<Vec<u8>, ProviderError>> {
    Box::pin(async_stream::try_stream! {
        let mut size = 0usize;
        while let Some(chunk) = body.next().await {
            let chunk = chunk?;
            size = size.checked_add(chunk.len()).filter(|size| *size <= MAX_RESPONSE_BYTES)
                .ok_or_else(|| protocol("Bedrock response exceeded its byte limit"))?;
            yield chunk;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_the_exact_bedrock_body_and_marks_credentials_sensitive() {
        for api in [OpenAiApi::ChatCompletions, OpenAiApi::Responses] {
            let credentials = Credentials::new(
                "fixture-key",
                "fixture-secret",
                Some("fixture-token".into()),
                None,
                "test",
            );
            let payload = br#"{"model":"us.moonshotai.kimi-k3","store":false}"#.to_vec();
            let request = signed_request(
                &Client::new(),
                "us-west-2",
                api,
                credentials,
                payload.clone(),
                true,
                SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            )
            .unwrap();
            assert_eq!(
                request.url().as_str(),
                format!(
                    "https://bedrock-runtime.us-west-2.amazonaws.com/openai/v1/{}",
                    api.path()
                )
            );
            assert_eq!(request.method(), "POST");
            assert_eq!(request.body().unwrap().as_bytes().unwrap(), payload);
            let authorization = &request.headers()["authorization"];
            assert!(authorization.is_sensitive());
            assert!(authorization
                .to_str()
                .unwrap()
                .contains("/us-west-2/bedrock/aws4_request"));
            assert!(request.headers()["x-amz-security-token"].is_sensitive());
            assert_eq!(request.headers()["accept"], "text/event-stream");
            let debug = format!("{request:?}");
            assert!(!debug.contains("fixture-token"));
            assert!(!debug.contains("fixture-key"));
        }
    }

    #[test]
    fn endpoint_cannot_escape_bedrock_or_guess_another_partition() {
        for api in [OpenAiApi::ChatCompletions, OpenAiApi::Responses] {
            for region in [
                "",
                "https://example.invalid",
                "us-west-2.example.invalid",
                "us-west-2/",
                "us-gov-west-1",
                "cn-north-1",
                "us-west-2?x=y",
            ] {
                assert!(endpoint(region, api).is_err(), "{region}");
            }
            assert!(endpoint("us-east-1", api).is_ok());
        }
    }
}
