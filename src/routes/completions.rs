use std::net::SocketAddr;

use tracing::error;
use serde_json::Value;
use axum::http::header;
use axum::{
    body::Body,
    extract::{ConnectInfo, Json, Request, State},
    http::{Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{
    CLIENT, COMPLETIONS_URL, DEFAULT_MODEL, delegates::error::APIError, is_allowed_model,
    metrics::database::MetricsState,
};

fn build_response_with_content_type(content_type: header::HeaderValue, body: impl Into<Body>) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(body.into())
        .expect("building response body should be infallible")
}

async fn log_request_response(
    state: &MetricsState,
    request: &Value,
    response: &Value,
    ip: std::net::IpAddr,
    is_streaming: bool,
) {
    let tokens = crate::metrics::database::extract_tokens(response, is_streaming);
    state.log_request(request, response, ip, tokens).await;
}

pub async fn validate_model(req: Request, next: Next) -> Result<Response, APIError> {
    let (parts, body) = req.into_parts();

    let body_bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|_| APIError {
            code: StatusCode::BAD_REQUEST,
            body: Some("Failed to read request body"),
        })?;

    let mut json: Value = serde_json::from_slice(&body_bytes).map_err(|_| APIError {
        code: StatusCode::BAD_REQUEST,
        body: Some("Invalid JSON"),
    })?;

    let selected = json
        .get("model")
        .and_then(Value::as_str)
        .filter(|&m| is_allowed_model(m))
        .unwrap_or(DEFAULT_MODEL)
        .to_string();
    json["model"] = Value::String(selected);

    let new_body = serde_json::to_vec(&json).map_err(|_| APIError {
        code: StatusCode::INTERNAL_SERVER_ERROR,
        body: Some("Failed to serialize request"),
    })?;

    let req = Request::from_parts(parts, axum::body::Body::from(new_body));

    Ok(next.run(req).await)
}

#[utoipa::path(
    post,
    path = "/chat/completions",
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Chat completion successful", body = serde_json::Value),
        (status = 400, description = "Bad request"),
        (status = 502, description = "Upstream service error")
    ),
    tag = "Chat",
    description = "OpenAI/Groq compatible chat completions endpoint. See: https://platform.openai.com/docs/api-reference/introduction and https://console.groq.com/docs/api-reference#chat-create"
)]
pub async fn completions(
    State(state): State<MetricsState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(request): Json<Value>,
) -> impl IntoResponse {
    let response = match CLIENT
        .request(Method::POST, COMPLETIONS_URL)
        .json(&request)
        .send()
        .await
    {
        Ok(response) => response,
        Err(e) => {
            error!("Failed to send request to Groq: {}", e);
            return APIError {
                code: StatusCode::BAD_GATEWAY,
                body: Some("Failed to connect to upstream service"),
            }
            .into_response();
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        return APIError {
            code: status,
            body: Some("Upstream service error"),
        }
        .into_response();
    }

    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap();

    let is_streaming = request
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if is_streaming {
        use futures::StreamExt;

        let ip = addr.ip();
        let state_clone = state.clone();
        let request_clone = request.clone();

        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        let mut final_usage: Option<Value> = None;

        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    buffer.extend_from_slice(&bytes);

                    let text = String::from_utf8_lossy(&bytes);
                    for line in text.lines() {
                        if let Some(json_part) = line.strip_prefix("data: ") {
                            if json_part != "[DONE]" {
                                if let Ok(parsed) = serde_json::from_str::<Value>(json_part) {
                                    if let Some(x_groq) = parsed.get("x_groq") {
                                        if x_groq.get("usage").is_some() {
                                            final_usage = Some(parsed.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }

        if let Some(final_response) = final_usage {
            log_request_response(&state_clone, &request_clone, &final_response, ip, true).await;
        }

        return build_response_with_content_type(content_type, buffer);
    }

    let response_body = match response.text().await {
        Ok(body) => body,
        Err(e) => {
            error!("Failed to read response body: {e}");
            return APIError {
                code: StatusCode::BAD_GATEWAY,
                body: Some("Failed to read upstream response"),
            }
            .into_response();
        }
    };

    let response_json: Value = match serde_json::from_str(&response_body) {
        Ok(json) => json,
        Err(e) => {
            error!("Failed to parse response JSON: {e}");
            return APIError {
                code: StatusCode::BAD_GATEWAY,
                body: Some("Invalid response from upstream service"),
            }
            .into_response();
        }
    };

    let ip = addr.ip();
    log_request_response(&state, &request, &response_json, ip, false).await;

    build_response_with_content_type(content_type, response_json.to_string())
}
