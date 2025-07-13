![](https://github.com/tyrchen/firecracker-poc/workflows/build/badge.svg)

# Firecracker POC

A proof-of-concept project demonstrating secure Python code execution in isolated microVMs using AWS Firecracker virtualization technology.

## Overview

This project provides a secure sandbox environment for executing Python code using Firecracker microVMs. It features:

- **Secure Isolation**: Python code execution in lightweight, secure microVMs
- **REST API**: Axum-based web server with clean JSON responses
- **Modern UI**: React/TypeScript frontend for interactive code execution
- **Cross-Platform**: Runs on macOS using Lima VMs with KVM support
- **Production Ready**: Comprehensive test suite with 16+ passing tests

## Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   React UI      │───▶│   Rust Backend  │───▶│  Firecracker    │
│   (TypeScript)  │    │   (Axum Server) │    │   microVMs      │
└─────────────────┘    └─────────────────┘    └─────────────────┘
                                │
                                ▼
                       ┌─────────────────┐
                       │   Lima VM       │
                       │   (x86_64)      │
                       └─────────────────┘
```

## Prerequisites

- **macOS**: Intel or Apple Silicon (project runs in x86_64 Lima VM)
- **Lima**: For VM management (`brew install lima`)
- **QEMU**: For x86_64 virtualization (`brew install qemu`)
- **Rust**: Latest stable version
- **Node.js & Yarn**: For UI development

## Setup

### 1. Clone and Build

```bash
git clone <repository-url>
cd firecracker-poc
cargo build
```

### 2. Create Lima VM

```bash
# Create x86_64 VM with KVM support
make create-linux

# Verify VM is running
limactl list
```

### 3. Setup Firecracker in VM

```bash
# SSH into the Lima VM
lima firecracker-vm

# Run setup script (inside VM)
./scripts/setup_firecracker.sh
```

### 4. Start the Server

```bash
# In the host system
cargo run
```

The server will start on `http://localhost:3000`

### 5. Start the UI (Optional)

```bash
cd ui
yarn install
yarn dev
```

The UI will be available on `http://localhost:5173`

## Usage

### API Endpoints

#### Execute Python Code

```bash
POST /execute
Content-Type: application/json

{
  "code": "print(2 + 2)"
}
```

**Response:**

```json
{
  "stdout": "4\n",
  "stderr": "",
  "success": true
}
```

#### Health Check

```bash
GET /health
```

**Response:**

```json
{
  "status": "healthy"
}
```

### Example Usage

```bash
# Simple calculation
curl -X POST http://localhost:3000/execute \
  -H "Content-Type: application/json" \
  -d '{"code": "result = 10 * 5\nprint(f\"Result: {result}\")"}'

# File operations (sandboxed)
curl -X POST http://localhost:3000/execute \
  -H "Content-Type: application/json" \
  -d '{"code": "import os\nprint(os.listdir(\"/\"))"}'

# Error handling
curl -X POST http://localhost:3000/execute \
  -H "Content-Type: application/json" \
  -d '{"code": "print(undefined_variable)"}'
```

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Lint code
cargo clippy
```

### Project Structure

```
├── src/                   # Rust backend source
│   ├── main.rs           # Server entry point
│   ├── lib.rs            # Library exports
│   └── runner.rs         # Firecracker integration
├── ui/                    # React frontend
│   ├── src/
│   └── components/
├── fixtures/              # Configuration files
│   └── machine.json      # Firecracker VM config
├── scripts/               # Setup and utility scripts
└── specs/                 # Project specifications
```

### Key Features

- **Clean Output**: Returns only Python execution results, no VM logs
- **Error Handling**: Comprehensive error reporting and recovery
- **Security**: Isolated execution environment with no host access
- **Performance**: Lightweight microVMs with fast startup times
- **Monitoring**: Health checks and status reporting

## Configuration

### Firecracker VM Settings

The VM configuration is stored in `fixtures/machine.json`:

```json
{
  "machine-config": {
    "vcpu_count": 1,
    "mem_size_mib": 256,
    "track_dirty_pages": true
  }
}
```

### Lima VM Configuration

The Lima VM is configured in `linux.yaml` for x86_64 with nested virtualization.

## Troubleshooting

### Common Issues

1. **KVM Access Denied**: Ensure user has KVM permissions in Lima VM
2. **CPU Template Errors**: Remove cpu_template from machine config for compatibility
3. **Architecture Mismatch**: Verify Lima VM is running x86_64 architecture
4. **Nested Virtualization**: Check that your system supports nested virtualization

### Debug Mode

Enable verbose logging:

```bash
RUST_LOG=debug cargo run
```

## Testing

The project includes comprehensive tests covering:

- API endpoint functionality
- Firecracker integration
- Error handling scenarios
- VM lifecycle management

All tests pass consistently across different environments.

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests: `cargo test && cargo clippy`
5. Submit a pull request

## License

This project is distributed under the terms of MIT.

See [LICENSE](LICENSE.md) for details.

Copyright 2025 Tyr Chen
