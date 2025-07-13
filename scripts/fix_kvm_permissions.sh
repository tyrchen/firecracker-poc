#!/bin/bash
set -e

echo "🔧 Fixing KVM permissions..."

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
echo "✅ KVM device permissions: $(ls -l /dev/kvm)"

# Test access
if test -r /dev/kvm && test -w /dev/kvm; then
    echo "✅ KVM access verified successfully"
else
    echo "⚠️  KVM permissions may need a group refresh. Try:"
    echo "   1. Log out and log back in, OR"
    echo "   2. Run: newgrp kvm"
    echo "   3. Then test again"
fi

echo "✅ KVM permissions fix complete"
