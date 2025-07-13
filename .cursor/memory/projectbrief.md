# Firecracker POC - Project Brief

## Project Overview
**Core Objective**: Create a Web service that provides a single API endpoint `/execute` to run Python code in secure Firecracker microVMs.

## Key Features
- **Single API Endpoint**: `/execute` accepts JSON with Python code
- **Dynamic VM Creation**: Fresh Firecracker microVM for each execution
- **Security**: Complete isolation for each code execution
- **Language**: Python (most common in AI/Agent field)
- **Output Capture**: Returns both stdout and stderr

## Technical Stack
- **Language**: Rust
- **Web Framework**: `axum` (modern, modular, tokio-integrated)
- **Async Runtime**: `tokio`
- **Serialization**: `serde`
- **HTTP Client**: `reqwest` (for Firecracker API communication)

## Core Workflow
1. Client sends POST to `http://localhost:3000/execute` with `{"code": "print(2 + 2)"}`
2. `axum` web service receives and parses request
3. Rust logic creates unique Firecracker configuration
4. Starts `firecracker` subprocess with API socket
5. Configures VM via HTTP API (machine-config, drives, boot-source)
6. Starts VM instance and injects Python code via stdin
7. Captures stdout/stderr output
8. Terminates VM and cleans up resources
9. Returns JSON response with execution results

## Key Components
- **VM Configuration**: Dynamic setup with unique IDs
- **API Socket Communication**: HTTP requests to Firecracker API
- **Process Management**: Subprocess spawning and cleanup
- **Resource Isolation**: Fresh VM per execution
- **Error Handling**: Comprehensive error capture

## Success Criteria
- Single API endpoint works correctly
- Python code executes in isolated microVM
- Output (stdout/stderr) correctly captured
- VM properly cleaned up after execution
- Response time acceptable for POC demonstration

## Next Steps Beyond POC
- VM pooling for performance
- Async task support with job IDs
- Enhanced security with jailer
- Improved error handling and timeouts
- Resource limits and monitoring
