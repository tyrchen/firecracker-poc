# Firecracker POC - Active Context

## Current Phase
**Mode**: BUILD (Implementation)
**Stage**: Core Data Structures Complete
**Focus**: Firecracker Integration Module

## Immediate Context
- **Project Type**: Rust-based web service for secure code execution
- **Architecture**: Axum web server + Firecracker microVM integration
- **Target**: Single `/execute` API endpoint for Python code execution
- **Security Model**: Complete isolation per execution via fresh microVMs

## Active Work Stream
1. **Memory Bank Initialization** ✅
   - Created `.cursor/memory/` structure
   - Established core documentation files
   - Defined project scope and requirements

2. **Core Data Structures** ✅
   - Implemented ExecuteRequest and ExecuteResponse structs
   - Created comprehensive ExecutionError enum
   - Added UUID support for VM identification
   - Built helper functions and comprehensive tests

3. **Next Immediate Steps**
   - Create runner.rs module for VM interaction
   - Implement Firecracker subprocess management
   - Add VM configuration and code execution logic

## Key Technical Decisions Made
- **Web Framework**: `axum` (modern, tokio-integrated)
- **VM Management**: Direct firecracker subprocess control
- **Communication**: HTTP API via Unix socket
- **Isolation**: Fresh VM per request (no pooling in POC)
- **Language Support**: Python only for POC

## Current Dependencies Status
```toml
[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
reqwest = { version = "0.12", features = ["json"] }
uuid = { version = "1", features = ["v4"] }
```

## Critical Success Factors
- VM lifecycle management (spawn → configure → execute → cleanup)
- Proper error handling and resource cleanup
- Secure process isolation
- Reliable stdout/stderr capture

## Current Blockers
- None identified

## Environment Requirements
- Firecracker binary available in PATH
- Linux kernel image: `./hello-vmlinux.bin`
- Root filesystem: `./alpine-python.ext4`
- Write permissions for socket creation in `/tmp/`

## Testing Strategy
- Unit tests for data structures
- Integration tests for VM lifecycle
- End-to-end API testing with curl
- Resource cleanup verification

## Next Session Focus
**Priority 1**: Firecracker integration module (runner.rs)
**Priority 2**: VM lifecycle management implementation
**Priority 3**: Basic web server setup with /execute endpoint
