use axum::{http::StatusCode, response::IntoResponse, response::Json};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::error;

pub mod runner;

// Re-export the main function for easy access
pub use runner::run_in_vm;

/// Request body for code execution
#[derive(Serialize, Deserialize)]
pub struct ExecuteRequest {
    /// Python code to execute in the microVM
    pub code: String,
}

/// Response structure for code execution results
#[derive(Debug, Serialize, Clone)]
pub struct ExecuteResponse {
    /// Standard output from the Python code execution
    pub stdout: String,
    /// Standard error from the Python code execution
    pub stderr: String,
    /// Whether the execution was successful
    pub success: bool,
}

#[derive(Error, Debug)]
pub enum ExecutionError {
    /// Error communicating with Firecracker API
    #[error("API communication error: {0}")]
    ApiCommunicationError(String),
    /// Execution timeout
    #[error("Execution timed out after 30 seconds")]
    TimeoutError,
    #[error("Execution timed out. Logs:\n{0}")]
    TimeoutErrorWithLogs(String),
    /// JSON serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),
    /// Resource-related error (socket, file system, etc.)
    #[error("Resource management error: {0}")]
    ResourceError(String),
    /// Error spawning a process
    #[error("Process spawning error: {0}")]
    ProcessSpawnError(String),
}

impl IntoResponse for ExecutionError {
    fn into_response(self) -> axum::response::Response {
        let status = match self {
            ExecutionError::ApiCommunicationError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ExecutionError::TimeoutError => StatusCode::INTERNAL_SERVER_ERROR,
            ExecutionError::TimeoutErrorWithLogs(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ExecutionError::SerializationError(_) => StatusCode::BAD_REQUEST,
            ExecutionError::ResourceError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ExecutionError::ProcessSpawnError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = Json(serde_json::json!({
            "error": self.to_string(),
        }));
        (status, body).into_response()
    }
}

/// Generate a unique VM identifier
pub fn generate_vm_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Create an ExecuteResponse for successful execution
pub fn create_success_response(stdout: String, stderr: String) -> ExecuteResponse {
    ExecuteResponse {
        stdout,
        stderr,
        success: true,
    }
}

/// Create an ExecuteResponse for failed execution
pub fn create_error_response(error_message: String) -> ExecuteResponse {
    ExecuteResponse {
        stdout: String::new(),
        stderr: error_message,
        success: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_request_deserialization() {
        let json = r#"{"code": "print('Hello, World!')"}"#;
        let request: ExecuteRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.code, "print('Hello, World!')");
    }

    #[test]
    fn test_execute_response_serialization() {
        let response = ExecuteResponse {
            stdout: "Hello, World!\n".to_string(),
            stderr: String::new(),
            success: true,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("Hello, World!"));
        assert!(json.contains("\"success\":true"));
    }

    #[test]
    fn test_vm_id_generation() {
        let id1 = generate_vm_id();
        let id2 = generate_vm_id();

        // IDs should be different
        assert_ne!(id1, id2);

        // IDs should be valid UUID format (36 characters with dashes)
        assert_eq!(id1.len(), 36);
        assert!(id1.contains('-'));
    }

    #[test]
    fn test_success_response_creation() {
        let response = create_success_response("output".to_string(), "".to_string());
        assert_eq!(response.stdout, "output");
        assert_eq!(response.stderr, "");
        assert!(response.success);
    }

    #[test]
    fn test_error_response_creation() {
        let response = create_error_response("error message".to_string());
        assert_eq!(response.stdout, "");
        assert_eq!(response.stderr, "error message");
        assert!(!response.success);
    }

    #[test]
    fn test_execution_error_display() {
        let error = ExecutionError::ProcessSpawnError("failed to start".to_string());
        assert_eq!(
            format!("{}", error),
            "Process spawning error: failed to start"
        );

        let timeout_error = ExecutionError::TimeoutError;
        assert_eq!(
            format!("{}", timeout_error),
            "Execution timed out after 30 seconds"
        );
    }
}
