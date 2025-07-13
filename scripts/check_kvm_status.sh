#!/bin/bash

echo "🔍 Checking KVM status in Lima VM..."

# Check if KVM device exists
if [ -e "/dev/kvm" ]; then
    echo "✅ /dev/kvm device found"
    ls -la /dev/kvm
else
    echo "❌ /dev/kvm device not found"
    echo "This usually means nested virtualization is not enabled."
    echo ""
    echo "To fix this:"
    echo "1. Stop the Lima VM: limactl stop firecracker-vm"
    echo "2. Start it again: limactl start firecracker-vm"
    echo "3. Re-run this script"
    exit 1
fi

# Check if KVM module is loaded
if lsmod | grep -q kvm; then
    echo "✅ KVM kernel module is loaded"
    lsmod | grep kvm
else
    echo "❌ KVM kernel module is not loaded"
    echo "Trying to load KVM module..."
    sudo modprobe kvm
    sudo modprobe kvm_intel  # For Intel processors
    sudo modprobe kvm_amd    # For AMD processors
fi

# Check CPU virtualization support
echo ""
echo "🔍 Checking CPU virtualization support..."
if grep -q "vmx\|svm" /proc/cpuinfo; then
    echo "✅ CPU supports virtualization"
    echo "CPU flags: $(grep -o "vmx\|svm" /proc/cpuinfo | head -1)"
else
    echo "❌ CPU does not support virtualization"
    echo "This may be because nested virtualization is not enabled."
fi

# Check if user can access KVM
echo ""
echo "🔍 Checking user access to /dev/kvm..."
if [ -r "/dev/kvm" ] && [ -w "/dev/kvm" ]; then
    echo "✅ User $(whoami) can access /dev/kvm"
else
    echo "❌ User $(whoami) cannot access /dev/kvm"
    echo "Running permissions fix..."
    ./scripts/fix_kvm_permissions.sh
fi

echo ""
echo "🎯 KVM Status Summary:"
echo "  - Device: $([ -e /dev/kvm ] && echo "✅ Found" || echo "❌ Missing")"
echo "  - Module: $(lsmod | grep -q kvm && echo "✅ Loaded" || echo "❌ Not loaded")"
echo "  - CPU: $(grep -q "vmx\|svm" /proc/cpuinfo && echo "✅ Supported" || echo "❌ Not supported")"
echo "  - Access: $([ -r /dev/kvm ] && [ -w /dev/kvm ] && echo "✅ Accessible" || echo "❌ No access")"
