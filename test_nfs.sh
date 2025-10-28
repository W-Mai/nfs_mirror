#!/bin/bash

# NFS Mirror Test Script

set -e

echo "=== NFS Mirror Functionality Test ==="

# Create test directory
TEST_DIR="/tmp/nfs_mirror_test"
MOUNT_DIR="/tmp/nfs_mount"

echo "1. Preparing test environment..."
rm -rf "$TEST_DIR" "$MOUNT_DIR"
mkdir -p "$TEST_DIR" "$MOUNT_DIR"

# Create some test files
echo "Hello NFS World" > "$TEST_DIR/test.txt"
echo "Rust NFS Server" > "$TEST_DIR/rust.txt"
mkdir -p "$TEST_DIR/subdir"
echo "Subdirectory file" > "$TEST_DIR/subdir/nested.txt"

echo "2. Starting NFS server..."
# Start NFS server in background
cargo run -- "$TEST_DIR" --port 12000 --verbose &
SERVER_PID=$!

# Wait for server to start
sleep 3

echo "3. Attempting to mount NFS share..."
# Try to mount (may require admin privileges)
if command -v mount >/dev/null 2>&1; then
    echo "Attempting to mount NFS share to $MOUNT_DIR..."
    if mount -t nfs -o nolocks,vers=3,tcp,port=12000,mountport=12000,soft 127.0.0.1:/ "$MOUNT_DIR" 2>/dev/null; then
        echo "Mount successful!"
        echo "4. Verifying file access..."
        ls -la "$MOUNT_DIR"
        echo "Test file contents:"
        cat "$MOUNT_DIR/test.txt" 2>/dev/null || echo "Unable to read test file"
        
        echo "5. Cleaning up mount..."
        umount "$MOUNT_DIR" 2>/dev/null || true
    else
        echo "Mount failed (may require admin privileges)"
    fi
else
    echo "mount command not available, skipping mount test"
fi

echo "6. Testing server status check..."
# Check if server is still running
if kill -0 $SERVER_PID 2>/dev/null; then
    echo "✓ NFS server is running normally"
else
    echo "✗ NFS server exited unexpectedly"
fi

echo "7. Cleaning up test environment..."
kill $SERVER_PID 2>/dev/null || true
rm -rf "$TEST_DIR" "$MOUNT_DIR"

echo "=== Test Complete ==="