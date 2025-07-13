use axum::{
    Router, extract::Json, http::StatusCode, response::Json as ResponseJson, routing::post,
};
use firecracker_poc::{ExecuteRequest, ExecuteResponse, create_error_response, run_in_vm};
use std::net::SocketAddr;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info};

/// Handler for the /execute endpoint
async fn execute_handler(
    Json(payload): Json<ExecuteRequest>,
) -> Result<ResponseJson<ExecuteResponse>, (StatusCode, ResponseJson<ExecuteResponse>)> {
    debug!("Received execute request with code: {}", payload.code);

    // Validate input
    if payload.code.trim().is_empty() {
        let error_response = create_error_response("Empty code provided".to_string());
        return Err((StatusCode::BAD_REQUEST, ResponseJson(error_response)));
    }

    // Check code length limit (prevent extremely large payloads)
    if payload.code.len() > 10_000 {
        let error_response =
            create_error_response("Code exceeds maximum length of 10,000 characters".to_string());
        return Err((StatusCode::BAD_REQUEST, ResponseJson(error_response)));
    }

    // Execute code in VM
    match run_in_vm(&payload.code).await {
        Ok(response) => {
            info!("Code execution completed successfully");
            Ok(ResponseJson(response))
        }
        Err(e) => {
            error!("Code execution failed: {}", e);
            let error_response = create_error_response(format!("Execution failed: {}", e));
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                ResponseJson(error_response),
            ))
        }
    }
}

/// Health check endpoint
async fn health_handler() -> &'static str {
    "OK"
}

/// Create the application router
fn create_app() -> Router {
    Router::new()
        .route("/execute", post(execute_handler))
        .route("/health", axum::routing::get(health_handler))
        .layer(ServiceBuilder::new().layer(TraceLayer::new_for_http()))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let app = create_app();

    // Bind to address
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("Firecracker POC server starting on {}", addr);

    // Create listener
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Server listening on http://{}", addr);
    info!("Available endpoints:");
    info!("  POST /execute - Execute Python code in secure microVM");
    info!("  GET  /health  - Health check endpoint");

    // Start server
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = create_app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_execute_endpoint_empty_code() {
        let app = create_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/execute")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"code": ""}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_execute_endpoint_too_long_code() {
        let app = create_app();
        let long_code = "a".repeat(10_001);

        let request_body = format!(r#"{{"code": "{}"}}"#, long_code);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/execute")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(request_body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_execute_endpoint_invalid_json() {
        let app = create_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/execute")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"invalid": json"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_execute_endpoint_missing_content_type() {
        let app = create_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/execute")
                    .body(Body::from(r#"{"code": "print('hello')"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn test_execute_endpoint_structure() {
        // This test verifies the endpoint structure without actual VM execution
        let app = create_app();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/execute")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"code": "print('test')"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        // The response should be either 200 (success) or 500 (VM execution failure)
        // Both are valid depending on whether Firecracker is available
        assert!(
            response.status() == StatusCode::OK
                || response.status() == StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
