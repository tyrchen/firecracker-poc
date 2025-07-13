# Firecracker POC - Technical Context

## Technology Stack

### Core Technologies
- **Language**: Rust 1.75+ (stable)
- **Web Framework**: `axum` 0.8 (async web framework)
- **Async Runtime**: `tokio` 1.0 (async runtime)
- **Virtualization**: Firecracker (microVM manager)
- **Serialization**: `serde` 1.0 (JSON handling)
- **HTTP Client**: `reqwest` 0.12 (Unix socket communication)
- **UUID Generation**: `uuid` 1.0 (unique VM identifiers)

### System Requirements
- **Operating System**: Linux (x86_64) - Firecracker dependency
- **Kernel**: Linux 4.14+ with KVM support
- **CPU**: Intel VT-x or AMD-V virtualization support
- **Memory**: 4GB+ RAM for host and VM allocations
- **Storage**: Local filesystem for VM images and temporary files

### Development Environment
- **Rust Toolchain**: rustc, cargo, rustfmt, clippy
- **Firecracker Binary**: Latest stable release
- **VM Images**: Linux kernel (vmlinux) and root filesystem (ext4)
- **Testing Tools**: curl, jq for API testing

## Architecture Constraints

### Platform Limitations
- **Linux Only**: Firecracker requires Linux KVM
- **Single Host**: POC limited to single machine deployment
- **Synchronous API**: Simple request-response pattern
- **No Persistence**: Stateless execution model

### Security Boundaries
- **Process Isolation**: Each VM runs in separate process
- **Resource Limits**: Memory and CPU quotas per VM
- **Network Isolation**: No network access from VMs
- **Filesystem Isolation**: Read-only root filesystem

### Performance Constraints
- **VM Startup Time**: ~100-500ms per VM initialization
- **Memory Overhead**: ~128MB minimum per VM
- **Process Limits**: Limited by system process limits
- **Socket Management**: Unix socket per VM instance

## Implementation Details

### Firecracker Configuration
```json
{
  "machine-config": {
    "vcpu_count": 1,
    "mem_size_mib": 128,
    "ht_enabled": false
  },
  "boot-source": {
    "kernel_image_path": "./hello-vmlinux.bin",
    "boot_args": "console=ttyS0 reboot=k panic=1 pci=off"
  },
  "drives": [{
    "drive_id": "rootfs",
    "path_on_host": "./alpine-python.ext4",
    "is_root_device": true,
    "is_read_only": false
  }]
}
```

### API Communication Pattern
```rust
// Unix socket communication
let socket_path = format!("/tmp/firecracker-{}.socket", vm_id);
let client = reqwest::Client::new();
let base_url = format!("http://unix/{}", socket_path);

// Configure VM
client.put(&format!("{}/machine-config", base_url))
    .json(&machine_config)
    .send().await?;
```

### Process Management
```rust
// Firecracker process spawning
let mut process = Command::new("firecracker")
    .arg("--api-sock")
    .arg(&socket_path)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;
```

## Data Flow

### Request Processing
1. **HTTP Request**: JSON payload with Python code
2. **Deserialization**: Parse request into `ExecuteRequest`
3. **VM Creation**: Generate unique ID and socket path
4. **Process Spawn**: Start Firecracker with API socket
5. **VM Configuration**: Send configuration via HTTP to socket
6. **Code Execution**: Inject Python code via stdin
7. **Output Capture**: Read stdout/stderr streams
8. **Cleanup**: Terminate process and remove socket
9. **Response**: Serialize result as JSON

### Error Handling Flow
```rust
pub enum ExecutionError {
    ProcessSpawnError(std::io::Error),
    ApiCommunicationError(reqwest::Error),
    TimeoutError,
    SerializationError(serde_json::Error),
    ResourceError(String),
}
```

## Resource Management

### VM Resource Allocation
- **Memory**: 128MB per VM (configurable)
- **CPU**: 1 vCPU per VM
- **Storage**: Temporary files cleaned up post-execution
- **Network**: No network access (isolated)

### Host Resource Considerations
- **File Descriptors**: Socket and pipe file descriptors per VM
- **Process Limits**: System process limits for concurrent VMs
- **Memory Usage**: Host memory for VM allocation
- **CPU Scheduling**: Host CPU sharing among VMs

### Cleanup Strategy
- **Automatic**: RAII pattern for resource cleanup
- **Explicit**: Drop implementation for VM manager
- **Timeout-based**: Forced cleanup after timeout
- **Error-resilient**: Cleanup even on failure paths

## Security Model

### Threat Model
- **Untrusted Code**: Python code from external sources
- **Host Protection**: Prevent access to host system
- **Resource Abuse**: Prevent resource exhaustion
- **Data Isolation**: No data leakage between executions

### Mitigation Strategies
- **VM Isolation**: Hardware-level isolation via KVM
- **Resource Limits**: CPU and memory quotas
- **Process Sandboxing**: Restricted system calls
- **Timeout Protection**: Execution time limits

## Testing Strategy

### Unit Tests
- **Data Structures**: Serialization/deserialization tests
- **Error Handling**: Error type conversion tests
- **Configuration**: Validation logic tests
- **Utilities**: Helper function tests

### Integration Tests
- **VM Lifecycle**: Full VM creation and cleanup
- **API Communication**: Firecracker API interaction
- **Process Management**: Process spawning and monitoring
- **Resource Cleanup**: Verify no resource leaks

### End-to-End Tests
- **API Testing**: HTTP request/response cycle
- **Error Scenarios**: Various failure modes
- **Performance**: Response time and resource usage
- **Concurrency**: Multiple simultaneous requests

## Development Workflow

### Build Process
```bash
# Development build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Code formatting
cargo fmt

# Linting
cargo clippy
```

### Quality Gates
- **Compilation**: Code must compile without warnings
- **Tests**: All tests must pass
- **Formatting**: Code must be formatted with rustfmt
- **Linting**: No clippy warnings
- **Documentation**: Public APIs must be documented

## Deployment Considerations

### System Dependencies
- **Firecracker Binary**: Must be in PATH or specified path
- **VM Images**: Kernel and filesystem images accessible
- **Permissions**: Sufficient permissions for KVM access
- **Network**: Available ports for HTTP service

### Configuration Management
- **Environment Variables**: Runtime configuration
- **Config Files**: Structured configuration
- **Defaults**: Sensible default values
- **Validation**: Configuration validation on startup

### Monitoring Requirements
- **Health Checks**: Service health endpoint
- **Metrics**: Performance and resource metrics
- **Logging**: Structured logging for debugging
- **Alerting**: Error rate and resource usage alerts

## Future Technical Considerations

### Scalability
- **Horizontal Scaling**: Multiple service instances
- **Load Balancing**: Request distribution
- **Resource Pooling**: Pre-warmed VM pools
- **Caching**: Response caching for common code

### Advanced Features
- **Multi-language Support**: Additional runtimes
- **Persistent Storage**: Optional state persistence
- **Network Access**: Controlled network connectivity
- **Custom Environments**: User-defined execution environments
