use axum::response::IntoResponse;

#[utoipa::path(
    get,
    path = "/model",
    responses(
        (status = 200, description = "Comma-delimited allowed model list", body = String)
    ),
    tag = "Legacy"
)]
pub async fn get_model() -> impl IntoResponse {
    crate::ALLOWED_MODELS
}

#[utoipa::path(
    get,
    path = "/echo",
    responses(
        (status = 200, description = "Greeting message", body = String)
    ),
    tag = "Legacy"
)]
pub async fn echo() -> impl IntoResponse {
    "Hey there!"
}

#[utoipa::path(
    get,
    path = "/hey",
    responses(
        (status = 200, description = "Hello message", body = String)
    ),
    tag = "Legacy"
)]
pub async fn manual_hello() -> impl IntoResponse {
    echo().await
}
