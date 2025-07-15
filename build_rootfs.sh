#!/bin/bash
set -e

echo "Building rootfs with Python API server..."

# Create working directory
WORK_DIR="/tmp/firecracker-rootfs-build"
ROOT_DIR="$WORK_DIR/rootfs"
OUTPUT_FILE="alpine-python-api.ext4"

# Clean up any existing build
rm -rf "$WORK_DIR"
mkdir -p "$ROOT_DIR"

# Check if we have the original Alpine rootfs
if [ ! -f "alpine-python.ext4" ]; then
    echo "Error: alpine-python.ext4 not found. Please ensure you have the base Alpine rootfs."
    exit 1
fi

# Mount the original rootfs to extract it
LOOP_DEVICE=$(sudo losetup -f)
sudo losetup "$LOOP_DEVICE" alpine-python.ext4

# Create mount point and mount
MOUNT_DIR="/tmp/firecracker-original-mount"
sudo mkdir -p "$MOUNT_DIR"
sudo mount "$LOOP_DEVICE" "$MOUNT_DIR"

# Copy everything from original rootfs
echo "Copying base rootfs..."
sudo cp -a "$MOUNT_DIR"/* "$ROOT_DIR"/ || true
sudo cp -a "$MOUNT_DIR"/.[!.]* "$ROOT_DIR"/ 2>/dev/null || true

# Unmount and detach
sudo umount "$MOUNT_DIR"
sudo losetup -d "$LOOP_DEVICE"
sudo rmdir "$MOUNT_DIR"

# Install Python in the rootfs
echo "Installing Python in Alpine rootfs..."

# Method 1: Check if Python is available in Lima environment and copy it
if command -v python3 >/dev/null 2>&1; then
    echo "Found Python3 in Lima environment, copying to rootfs..."
    PYTHON_PATH=$(which python3)
    echo "Python path: $PYTHON_PATH"

    # Copy Python binary
    sudo cp "$PYTHON_PATH" "$ROOT_DIR/usr/bin/python3"
    sudo chmod +x "$ROOT_DIR/usr/bin/python3"

    # Copy Python libraries if they exist
    for lib_dir in /usr/lib/python3.* /usr/local/lib/python3.*; do
        if [ -d "$lib_dir" ]; then
            echo "Copying Python library: $lib_dir"
            sudo mkdir -p "$ROOT_DIR/usr/lib/"
            sudo cp -r "$lib_dir" "$ROOT_DIR/usr/lib/" 2>/dev/null || true
        fi
    done

    # Copy essential shared libraries that Python depends on
    echo "Copying Python shared libraries..."

    # Create necessary library directories
    sudo mkdir -p "$ROOT_DIR/lib/x86_64-linux-gnu/"
    sudo mkdir -p "$ROOT_DIR/usr/lib/x86_64-linux-gnu/"
    sudo mkdir -p "$ROOT_DIR/lib64/"

    # Copy Python-specific libraries
    for lib in /lib/x86_64-linux-gnu/libpython3.*.so.* /usr/lib/x86_64-linux-gnu/libpython3.*.so.*; do
        if [ -f "$lib" ]; then
            echo "Copying Python library: $lib"
            sudo cp "$lib" "$ROOT_DIR/lib/x86_64-linux-gnu/" 2>/dev/null || true
        fi
    done

    # Copy essential system libraries that Python depends on
    for lib in \
        /lib/x86_64-linux-gnu/libc.so.* \
        /lib/x86_64-linux-gnu/libdl.so.* \
        /lib/x86_64-linux-gnu/libm.so.* \
        /lib/x86_64-linux-gnu/libz.so.* \
        /lib/x86_64-linux-gnu/libexpat.so.* \
        /lib/x86_64-linux-gnu/libssl.so.* \
        /lib/x86_64-linux-gnu/libcrypto.so.* \
        /lib/x86_64-linux-gnu/libffi.so.* \
        /lib/x86_64-linux-gnu/libbz2.so.* \
        /lib/x86_64-linux-gnu/liblzma.so.* \
        /lib/x86_64-linux-gnu/libncursesw.so.* \
        /lib/x86_64-linux-gnu/libtinfo.so.* \
        /lib/x86_64-linux-gnu/libreadline.so.* \
        /lib/x86_64-linux-gnu/libsqlite3.so.* \
        /lib/x86_64-linux-gnu/libuuid.so.* \
        /lib/x86_64-linux-gnu/libpthread.so.* \
        /lib/x86_64-linux-gnu/librt.so.* \
        /lib/x86_64-linux-gnu/libnsl.so.* \
        /lib/x86_64-linux-gnu/libresolv.so.* \
        /lib/x86_64-linux-gnu/libutil.so.* \
        /lib/x86_64-linux-gnu/libgcc_s.so.* \
        /usr/lib/x86_64-linux-gnu/libffi.so.* \
        /usr/lib/x86_64-linux-gnu/libssl.so.* \
        /usr/lib/x86_64-linux-gnu/libcrypto.so.* \
        /usr/lib/x86_64-linux-gnu/libexpat.so.* \
        /usr/lib/x86_64-linux-gnu/libz.so.* \
        ; do
        if [ -f "$lib" ]; then
            echo "Copying system library: $lib"
            sudo cp "$lib" "$ROOT_DIR/lib/x86_64-linux-gnu/" 2>/dev/null || true
        fi
    done

    # Copy the dynamic linker
    if [ -f "/lib64/ld-linux-x86-64.so.2" ]; then
        echo "Copying dynamic linker..."
        sudo cp "/lib64/ld-linux-x86-64.so.2" "$ROOT_DIR/lib64/" 2>/dev/null || true
    fi

    echo "Python installation completed using Lima environment"

elif command -v python >/dev/null 2>&1; then
    echo "Found Python in Lima environment, copying to rootfs..."
    PYTHON_PATH=$(which python)
    echo "Python path: $PYTHON_PATH"

    # Copy Python binary
    sudo cp "$PYTHON_PATH" "$ROOT_DIR/usr/bin/python"
    sudo chmod +x "$ROOT_DIR/usr/bin/python"

    # Also create a python3 symlink
    sudo ln -sf python "$ROOT_DIR/usr/bin/python3"

    echo "Python installation completed using Lima environment (python2/3)"

else
    echo "Method 2: Installing Python using package manager in Lima..."

    # Try to install Python using the package manager available in Lima
    if command -v apt-get >/dev/null 2>&1; then
        echo "Using apt-get to install Python..."
        sudo apt-get update -qq
        sudo apt-get install -y python3 python3-minimal

        # Copy from system to rootfs
        sudo cp /usr/bin/python3 "$ROOT_DIR/usr/bin/python3"
        sudo chmod +x "$ROOT_DIR/usr/bin/python3"

        # Copy essential libraries
        sudo mkdir -p "$ROOT_DIR/usr/lib/"
        sudo cp -r /usr/lib/python3.* "$ROOT_DIR/usr/lib/" 2>/dev/null || true

    elif command -v yum >/dev/null 2>&1; then
        echo "Using yum to install Python..."
        sudo yum install -y python3

        # Copy from system to rootfs
        sudo cp /usr/bin/python3 "$ROOT_DIR/usr/bin/python3"
        sudo chmod +x "$ROOT_DIR/usr/bin/python3"

    else
        echo "ERROR: No Python found and no supported package manager available"
        echo "Please install Python manually or use a different approach"
        exit 1
    fi

    echo "Python installation completed using package manager"
fi

# Add our API server
echo "Adding VM API server..."
sudo cp vm_api_server.py "$ROOT_DIR/usr/local/bin/vm_api_server.py"
sudo chmod +x "$ROOT_DIR/usr/local/bin/vm_api_server.py"

# Create systemd service file to start the API server
sudo mkdir -p "$ROOT_DIR/etc/systemd/system"
sudo tee "$ROOT_DIR/etc/systemd/system/vm-api-server.service" > /dev/null << 'EOF'
[Unit]
Description=VM API Server
After=network.target

[Service]
Type=simple
User=root
ExecStart=/usr/bin/python3 /usr/local/bin/vm_api_server.py
Restart=always
RestartSec=1
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

# Enable the service
sudo chroot "$ROOT_DIR" systemctl enable vm-api-server.service 2>/dev/null || {
    echo "Systemctl not available, creating init script instead..."

    # Create init.d directory if it doesn't exist
    sudo mkdir -p "$ROOT_DIR/etc/init.d"

    # Create init script for systems without systemd
    sudo tee "$ROOT_DIR/etc/init.d/vm-api-server" > /dev/null << 'EOF'
#!/bin/sh
### BEGIN INIT INFO
# Provides:          vm-api-server
# Required-Start:    $network $local_fs
# Required-Stop:     $network $local_fs
# Default-Start:     2 3 4 5
# Default-Stop:      0 1 6
# Short-Description: VM API Server
# Description:       Python API server for code execution
### END INIT INFO

case "$1" in
    start)
        echo "Starting VM API Server..."
        /usr/bin/python3 /usr/local/bin/vm_api_server.py &
        echo $! > /var/run/vm-api-server.pid
        ;;
    stop)
        echo "Stopping VM API Server..."
        if [ -f /var/run/vm-api-server.pid ]; then
            kill $(cat /var/run/vm-api-server.pid)
            rm /var/run/vm-api-server.pid
        fi
        ;;
    restart)
        $0 stop
        $0 start
        ;;
    *)
        echo "Usage: $0 {start|stop|restart}"
        exit 1
        ;;
esac
EOF

    sudo chmod +x "$ROOT_DIR/etc/init.d/vm-api-server"

    # Create rc.local if it doesn't exist and add API server startup
    if [ ! -f "$ROOT_DIR/etc/rc.local" ]; then
        sudo tee "$ROOT_DIR/etc/rc.local" > /dev/null << 'EOF'
#!/bin/sh
# rc.local - executed at the end of each multiuser runlevel
EOF
    fi

    # Add to rc.local for automatic startup
    sudo tee -a "$ROOT_DIR/etc/rc.local" > /dev/null << 'EOF'
# Start VM API Server
/usr/bin/python3 /usr/local/bin/vm_api_server.py &
EOF

    sudo chmod +x "$ROOT_DIR/etc/rc.local"
}

# Create a simple startup script that will be run by init
sudo tee "$ROOT_DIR/usr/local/bin/startup.sh" > /dev/null << 'EOF'
#!/bin/sh
# Wait for network to be ready
sleep 2

# Configure network interface (IP should be configured by kernel cmdline, but ensure it's up)
ip link set eth0 up

# Find python and start the API server
PYTHON_CMD=""
if command -v python3 > /dev/null 2>&1; then
    PYTHON_CMD="python3"
elif command -v python > /dev/null 2>&1; then
    PYTHON_CMD="python"
elif [ -x /usr/bin/python3 ]; then
    PYTHON_CMD="/usr/bin/python3"
elif [ -x /usr/bin/python ]; then
    PYTHON_CMD="/usr/bin/python"
else
    echo "ERROR: Python not found!" > /dev/console
    echo "Available in /usr/bin/:" > /dev/console
    ls -la /usr/bin/ | grep python > /dev/console 2>&1 || echo "No python executables found" > /dev/console
    exit 1
fi

echo "Starting VM API server with: $PYTHON_CMD" > /dev/console
$PYTHON_CMD /usr/local/bin/vm_api_server.py &
API_PID=$!
echo "VM API server started (PID: $API_PID)" > /dev/console

# Keep the system running
tail -f /dev/null
EOF

sudo chmod +x "$ROOT_DIR/usr/local/bin/startup.sh"

# Calculate the size needed for the new rootfs
SIZE_KB=$(sudo du -s "$ROOT_DIR" | cut -f1)
SIZE_MB=$((SIZE_KB / 1024 + 100))  # Add 100MB buffer

echo "Creating new rootfs image (${SIZE_MB}MB)..."

# Create the new ext4 image
dd if=/dev/zero of="$OUTPUT_FILE" bs=1M count="$SIZE_MB"
mkfs.ext4 -F "$OUTPUT_FILE"

# Mount the new image and copy our rootfs
LOOP_DEVICE=$(sudo losetup -f)
sudo losetup "$LOOP_DEVICE" "$OUTPUT_FILE"

MOUNT_DIR="/tmp/firecracker-new-mount"
sudo mkdir -p "$MOUNT_DIR"
sudo mount "$LOOP_DEVICE" "$MOUNT_DIR"

echo "Copying modified rootfs to new image..."
sudo cp -a "$ROOT_DIR"/* "$MOUNT_DIR"/
sudo cp -a "$ROOT_DIR"/.[!.]* "$MOUNT_DIR"/ 2>/dev/null || true

# Unmount and cleanup
sudo umount "$MOUNT_DIR"
sudo losetup -d "$LOOP_DEVICE"
sudo rmdir "$MOUNT_DIR"

# Clean up build directory
sudo rm -rf "$WORK_DIR"

echo "Successfully created $OUTPUT_FILE with VM API server"
echo "The VM will run a Python API server on port 8080"
echo ""
echo "Endpoints:"
echo "  GET  /health   - Health check"
echo "  POST /execute  - Execute Python code"
echo "  POST /shutdown - Shutdown VM"
