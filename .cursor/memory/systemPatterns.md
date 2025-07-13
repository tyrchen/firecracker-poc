# Firecracker POC - System Patterns

## Architecture Overview

### High-Level Pattern
**Layered Architecture** with clear separation of concerns:
- **Web Layer**: HTTP API handling (axum)
- **Service Layer**: Business logic and orchestration
- **VM Layer**: Firecracker microVM management
- **Process Layer**: System process and resource management

### Component Interaction Pattern
```
[Client] → [HTTP API] → [Service Layer] → [VM Manager] → [Firecracker Process]
    ↓         ↓              ↓               ↓              ↓
[Response] ← [JSON] ← [Result Processing] ← [Output Capture] ← [VM Execution]
```

## Core Design Patterns

### 1. Resource Management Pattern
- **RAII (Resource Acquisition Is Initialization)**
- **Scope-based Cleanup**: Resources cleaned up when going out of scope
- **Unique VM IDs**: Each execution gets fresh, isolated environment
- **Automatic Cleanup**: VM processes terminated on completion/error

### 2. Error Handling Pattern
- **Result Type**: Rust's `Result<T, E>` for error propagation
- **Structured Errors**: Custom error types for different failure modes
- **Graceful Degradation**: Service continues after individual execution failures
- **Error Context**: Clear error messages with context information

### 3. Async Processing Pattern
- **Tokio Runtime**: Async/await for non-blocking operations
- **Process Management**: Async process spawning and monitoring
- **Stream Processing**: Async reading of stdout/stderr streams
- **Timeout Handling**: Async timeouts for VM operations

### 4. Communication Pattern
- **HTTP over Unix Socket**: Firecracker API communication
- **JSON Serialization**: Structured data exchange
- **Process Pipes**: stdin/stdout/stderr for code execution
- **RESTful API**: Simple HTTP POST endpoint

## Implementation Patterns

### Data Structure Pattern
```rust
// Request/Response Types
#[derive(Deserialize)]
struct ExecuteRequest {
    code: String,
}

#[derive(Serialize)]
struct ExecuteResponse {
    stdout: String,
    stderr: String,
    success: bool,
}
```

### Service Layer Pattern
```rust
// Service orchestration
pub struct ExecutionService {
    // Configuration and state
}

impl ExecutionService {
    pub async fn execute_code(&self, request: ExecuteRequest) -> Result<ExecuteResponse, ExecutionError> {
        // 1. Generate unique VM ID
        // 2. Create VM configuration
        // 3. Start Firecracker process
        // 4. Configure VM via API
        // 5. Execute code
        // 6. Capture output
        // 7. Cleanup resources
    }
}
```

### VM Management Pattern
```rust
// VM lifecycle management
pub struct VMManager {
    vm_id: String,
    process: Child,
    socket_path: String,
}

impl VMManager {
    pub async fn new() -> Result<Self, VMError> { /* ... */ }
    pub async fn configure(&self) -> Result<(), VMError> { /* ... */ }
    pub async fn execute(&self, code: &str) -> Result<ExecutionResult, VMError> { /* ... */ }
    pub async fn cleanup(self) -> Result<(), VMError> { /* ... */ }
}
```

## Security Patterns

### 1. Process Isolation
- **Separate User**: Run Firecracker with restricted permissions
- **Process Sandboxing**: Limit system calls and resources
- **Temporary Resources**: Use temporary files and cleanup
- **No Network Access**: Isolated network namespace

### 2. Resource Control
- **Memory Limits**: Configure VM memory allocation
- **CPU Limits**: Control CPU usage per VM
- **Time Limits**: Execution timeouts to prevent infinite loops
- **File System**: Read-only root filesystem

### 3. Input Validation
- **Code Sanitization**: Basic validation of input code
- **Size Limits**: Maximum code size limitations
- **Character Filtering**: Remove dangerous characters/sequences
- **Format Validation**: JSON structure validation

## Performance Patterns

### 1. Efficient Resource Usage
- **Quick VM Startup**: Minimal VM configuration
- **Fast Cleanup**: Efficient process termination
- **Memory Management**: Proper cleanup of resources
- **Stream Processing**: Efficient stdout/stderr handling

### 2. Concurrent Execution
- **Async Operations**: Non-blocking operations throughout
- **Parallel Processing**: Multiple VMs can run simultaneously
- **Resource Pooling**: (Future) Pre-warmed VM pools
- **Load Balancing**: (Future) Distribute across multiple hosts

## Error Recovery Patterns

### 1. Graceful Failure
- **Cleanup on Error**: Resources cleaned up even on failure
- **Error Propagation**: Clear error messages to client
- **Service Resilience**: Service continues after individual failures
- **Resource Leak Prevention**: Proper resource cleanup in all paths

### 2. Timeout Handling
- **Execution Timeouts**: Prevent infinite loops
- **Startup Timeouts**: Handle VM startup failures
- **API Timeouts**: Network operation timeouts
- **Cleanup Timeouts**: Ensure cleanup completes

## Monitoring and Observability Patterns

### 1. Structured Logging
- **Log Levels**: Debug, Info, Warn, Error levels
- **Structured Data**: JSON-formatted logs
- **Correlation IDs**: Track requests across components
- **Performance Metrics**: Execution time tracking

### 2. Health Checks
- **Service Health**: Basic health endpoint
- **Resource Health**: Monitor system resources
- **VM Health**: Track VM lifecycle metrics
- **Error Rates**: Monitor failure rates

## Configuration Patterns

### 1. Environment-based Configuration
- **VM Configuration**: CPU, memory, timeout settings
- **File Paths**: Kernel and filesystem paths
- **Network Settings**: Socket paths and ports
- **Security Settings**: User permissions and limits

### 2. Default Values
- **Sensible Defaults**: Good defaults for POC usage
- **Override Capability**: Environment variables for customization
- **Validation**: Configuration validation on startup
- **Documentation**: Clear configuration documentation

## Testing Patterns

### 1. Unit Testing
- **Data Structure Tests**: JSON serialization/deserialization
- **Error Handling Tests**: Error type conversion and propagation
- **Configuration Tests**: Validation of configuration parsing
- **Utility Function Tests**: Helper function testing

### 2. Integration Testing
- **VM Lifecycle Tests**: Full VM creation and cleanup
- **API Communication Tests**: Firecracker API interaction
- **Process Management Tests**: Process spawning and monitoring
- **Resource Cleanup Tests**: Verify no resource leaks

### 3. End-to-End Testing
- **Full API Tests**: Complete HTTP request/response cycle
- **Error Scenario Tests**: Various failure modes
- **Performance Tests**: Response time and resource usage
- **Concurrent Execution Tests**: Multiple simultaneous requests
