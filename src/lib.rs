use serde::{Deserialize, Serialize};
use std::fmt;

pub mod runner;

// Re-export the main function for easy access
pub use runner::run_in_vm;

/// Request structure for code execution
#[derive(Debug, Deserialize, Clone)]
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

/// Errors that can occur during code execution
#[derive(Debug, Clone)]
pub enum ExecutionError {
    /// Error spawning the Firecracker process
    ProcessSpawnError(String),
    /// Error communicating with Firecracker API
    ApiCommunicationError(String),
    /// Execution timeout
    TimeoutError,
    /// JSON serialization/deserialization error
    SerializationError(String),
    /// Resource-related error (socket, file system, etc.)
    ResourceError(String),
    /// VM configuration error
    ConfigurationError(String),
    /// VM startup error
    StartupError(String),
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecutionError::ProcessSpawnError(msg) => write!(f, "Process spawn error: {}", msg),
            ExecutionError::ApiCommunicationError(msg) => {
                write!(f, "API communication error: {}", msg)
            }
            ExecutionError::TimeoutError => write!(f, "Execution timeout"),
            ExecutionError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            ExecutionError::ResourceError(msg) => write!(f, "Resource error: {}", msg),
            ExecutionError::ConfigurationError(msg) => write!(f, "Configuration error: {}", msg),
            ExecutionError::StartupError(msg) => write!(f, "VM startup error: {}", msg),
        }
    }
}

impl std::error::Error for ExecutionError {}

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
        assert_eq!(format!("{}", error), "Process spawn error: failed to start");

        let timeout_error = ExecutionError::TimeoutError;
        assert_eq!(format!("{}", timeout_error), "Execution timeout");
    }
}
