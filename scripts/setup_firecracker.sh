#!/bin/bash
set -e

sudo apt-get update
sudo apt-get install -y build-essential

# Install Rust if not already installed
if ! command -v rustc &> /dev/null; then
    echo "🦀 Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source ~/.cargo/env
    echo "✅ Rust installed successfully"
else
    echo "✅ Rust is already installed"
    rustc --version
fi

echo "🔥 Setting up Firecracker POC environment..."

# Check architecture
ARCH=$(uname -m)
echo "Detected architecture: $ARCH"

# Download and install Firecracker
echo "📥 Downloading Firecracker..."
if [ "$ARCH" = "x86_64" ]; then
    FIRECRACKER_URL="https://github.com/firecracker-microvm/firecracker/releases/download/v1.12.1/firecracker-v1.12.1-x86_64.tgz"
elif [ "$ARCH" = "aarch64" ]; then
    FIRECRACKER_URL="https://github.com/firecracker-microvm/firecracker/releases/download/v1.12.1/firecracker-v1.12.1-aarch64.tgz"
else
    echo "❌ Unsupported architecture: $ARCH"
    exit 1
fi

# Create temp directory and download
TEMP_DIR=$(mktemp -d)
cd "$TEMP_DIR"
wget "$FIRECRACKER_URL" -O firecracker.tgz
tar -xzf firecracker.tgz

# Install to /usr/local/bin
echo "📦 Installing Firecracker to /usr/local/bin..."
sudo cp release-*/firecracker-* /usr/local/bin/
sudo chmod +x /usr/local/bin/firecracker-*

# Create symlinks
if [ "$ARCH" = "x86_64" ]; then
    sudo ln -sf /usr/local/bin/firecracker-v1.12.1-x86_64 /usr/local/bin/firecracker
elif [ "$ARCH" = "aarch64" ]; then
    sudo ln -sf /usr/local/bin/firecracker-v1.12.1-aarch64 /usr/local/bin/firecracker
fi

# Cleanup
cd - > /dev/null
rm -rf "$TEMP_DIR"

# Verify installation
echo "✅ Verifying Firecracker installation..."
firecracker --version

# Setup KVM permissions
echo "🔧 Setting up KVM permissions..."

# Check if KVM is available
if [ ! -e "/dev/kvm" ]; then
    echo "❌ KVM device not found. Please ensure KVM is enabled in your system."
    exit 1
fi

# Check if kvm group exists, create if not
if ! getent group kvm > /dev/null; then
    echo "Creating kvm group..."
    sudo groupadd kvm
fi

# Add current user to kvm group
echo "Adding user $(whoami) to kvm group..."
sudo usermod -aG kvm $(whoami)

# Set proper permissions on /dev/kvm
echo "Setting permissions on /dev/kvm..."
sudo chmod 666 /dev/kvm
sudo chown root:kvm /dev/kvm

# Verify KVM access
echo "✅ KVM setup complete. Current user: $(whoami)"
echo "✅ KVM device permissions: $(ls -l /dev/kvm)"
echo "✅ User groups: $(groups)"

# Note about group changes
echo "⚠️  Note: If you just added to kvm group, you may need to:"
echo "   - Log out and log back in, OR"
echo "   - Run 'newgrp kvm' to activate the group"

# Check required files
echo "📁 Checking required files..."
if [ ! -f "hello-vmlinux.bin" ]; then
    echo "❌ Missing hello-vmlinux.bin"
    exit 1
fi

if [ ! -f "alpine-python.ext4" ]; then
    echo "❌ Missing alpine-python.ext4"
    exit 1
fi

echo "✅ All required files present"

# Test KVM access
echo "🧪 Testing KVM access..."
if ! test -r /dev/kvm; then
    echo "❌ Cannot read /dev/kvm. You may need to:"
    echo "   1. Log out and log back in"
    echo "   2. Or run: newgrp kvm"
    echo "   3. Then re-run this script"
    echo ""
    echo "You can also try running: sudo chmod 666 /dev/kvm"
    exit 1
fi

if ! test -w /dev/kvm; then
    echo "❌ Cannot write to /dev/kvm. You may need to:"
    echo "   1. Log out and log back in"
    echo "   2. Or run: newgrp kvm"
    echo "   3. Then re-run this script"
    echo ""
    echo "You can also try running: sudo chmod 666 /dev/kvm"
    exit 1
fi

echo "✅ KVM access verified successfully"
