# Firecracker POC - Active Context

## Current Phase
**Mode**: VAN (Initialization)
**Stage**: Memory Bank Setup
**Focus**: Project Foundation & Structure

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

2. **Next Immediate Steps**
   - Verify current Cargo.toml dependencies
   - Define core data structures (ExecuteRequest, ExecuteResponse)
   - Create Firecracker integration module outline

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
**Priority 1**: Core data structures and type definitions
**Priority 2**: Firecracker integration module skeleton
**Priority 3**: Basic web server setup
