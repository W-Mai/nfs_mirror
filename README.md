# NFS Mirror

A high-performance NFS (Network File System) server implementation in Rust that mirrors local directories to NFS shared
services.

## Features

- 🚀 High-performance async NFS server
- 🔧 Rich CLI configuration options
- 🛡️ Read-only mode support
- 🌐 Flexible network configuration
- 📝 Configurable log levels
- 🔄 Daemon mode support
- 🎯 IP access control
- ⚡ Timeout configuration

## Installation

```bash
cargo install --path .
```

## Usage

### Basic Usage

```bash
# Start NFS mirror service
nfs_mirror /path/to/directory

# Specify IP and port
nfs_mirror /path/to/directory --ip 0.0.0.0 --port 2049

# Enable verbose output
nfs_mirror /path/to/directory --verbose
```

### Advanced Configuration

```bash
# Complete configuration example
nfs_mirror /path/to/directory \
    --ip 192.168.1.100 \
    --port 2049 \
    --log-level info \
    --verbose \
    --max-connections 200 \
    --read-timeout 60 \
    --write-timeout 60 \
    --read-only \
    --allow-ips "192.168.1.0/24,10.0.0.100"
```

### Daemon Mode

```bash
# Run in background
nfs_mirror /path/to/directory --daemon --pid-file /var/run/nfs_mirror.pid

# Specify working directory
nfs_mirror /path/to/directory --daemon --work-dir /var/lib/nfs_mirror
```

## CLI Parameters

### Required Parameters

- `<DIRECTORY>`: Local directory path to mirror

### Optional Parameters

#### Network Configuration

- `-i, --ip <IP>`: Listen IP address (default: 127.0.0.1)
- `-p, --port <PORT>`: Listen port (default: 11451)
- `--allow-ips <ALLOW_IPS>`: Comma-separated list of allowed client IP addresses

#### Log Configuration

- `-l, --log-level <LOG_LEVEL>`: Log level (default: error)
    - Available values: trace, debug, info, warn, error
- `-v, --verbose`: Enable verbose output
- `--no-color`: Disable log colors

#### Runtime Mode

- `-d, --daemon`: Run in daemon mode
- `--read-only`: Enable read-only mode
- `--pid-file <PID_FILE>`: PID file path (used in daemon mode)
- `--work-dir <WORK_DIR>`: Working directory

#### Performance Configuration

- `--max-connections <MAX_CONNECTIONS>`: Maximum connections (default: 100)
- `--read-timeout <READ_TIMEOUT>`: Read timeout in seconds (default: 30)
- `--write-timeout <WRITE_TIMEOUT>`: Write timeout in seconds (default: 30)

#### Help Information

- `-h, --help`: Display help information
- `-V, --version`: Display version information

## Client Mounting

### Linux Client

```bash
# Create mount point
sudo mkdir /mnt/nfs

# Mount NFS share
sudo mount -t nfs -o nolocks,vers=3,tcp,port=11451,mountport=11451,soft 127.0.0.1:/ /mnt/nfs/

# Unmount
sudo umount /mnt/nfs
```

### macOS Client

```bash
# Create mount point
sudo mkdir /mnt/nfs

# Mount NFS share
sudo mount -t nfs -o resvport,nolocks,vers=3,tcp,port=11451,mountport=11451 127.0.0.1:/ /mnt/nfs

# Unmount
sudo umount /mnt/nfs
```

## Configuration Examples

### Development Environment

```bash
nfs_mirror ./dev_data \
    --verbose \
    --log-level debug \
    --ip 127.0.0.1 \
    --port 11451
```

### Production Environment

```bash
nfs_mirror /data/shared \
    --daemon \
    --pid-file /var/run/nfs_mirror.pid \
    --work-dir /var/lib/nfs_mirror \
    --ip 0.0.0.0 \
    --port 2049 \
    --log-level warn \
    --max-connections 500 \
    --read-timeout 120 \
    --write-timeout 120 \
    --allow-ips "10.0.0.0/8,192.168.0.0/16"
```

### Read-only Share

```bash
nfs_mirror /public/files \
    --read-only \
    --ip 0.0.0.0 \
    --port 2049 \
    --max-connections 1000
```

## Security Considerations

1. **Network Access**: Use `--allow-ips` to restrict client access
2. **Read-only Mode**: Enable `--read-only` for scenarios that don't require writing
3. **Firewall**: Ensure firewall rules are properly configured
4. **Permissions**: Ensure the mirrored directory permissions are set correctly

## Testing

The project includes a comprehensive test script to verify NFS server functionality:

```bash
# Run the test script
./test_nfs.sh
```

The test script will:

1. Create a test directory with sample files
2. Start the NFS server in background
3. Attempt to mount the NFS share (requires appropriate permissions)
4. Verify file access and functionality
5. Clean up test environment

### Manual Testing

```bash
# Create test directory
mkdir -p /tmp/nfs_test
echo "Hello NFS" > /tmp/nfs_test/test.txt

# Start server (in one terminal)
cargo run -- /tmp/nfs_test --port 12000 --verbose

# Test mount (in another terminal, may require sudo)
mkdir -p /tmp/nfs_mount
mount -t nfs -o nolocks,vers=3,tcp,port=12000,mountport=12000,soft 127.0.0.1:/ /tmp/nfs_mount

# Verify files
ls -la /tmp/nfs_mount
cat /tmp/nfs_mount/test.txt

# Cleanup
umount /tmp/nfs_mount
```

## Troubleshooting

### Common Issues

1. **Port Occupied**: Change the port number or stop the process occupying the port
2. **Insufficient Permissions**: Ensure you have read permissions for the mirrored directory
3. **Network Connection**: Check firewall and network configuration
4. **Mount Failure**: Ensure NFS client is installed and properly configured
5. **macOS NFS Issues**: On macOS, you may need to use `resvport` option and ensure NFS client is enabled

### Debug Mode

```bash
# Enable verbose logging for debugging
nfs_mirror /path/to/directory --verbose --log-level debug
```

### Log Analysis

The server provides detailed logging when `--verbose` is enabled:

- Connection status and client information
- File system operations
- Performance metrics
- Error conditions

## Performance Considerations

- **Async I/O**: The server uses Tokio for high-performance asynchronous I/O
- **Connection Pooling**: Configurable maximum connections to balance performance and resource usage
- **Timeout Configuration**: Adjustable read/write timeouts for different network conditions
- **Memory Efficiency**: Symbol table for efficient path handling

## License

This project is licensed under [LICENSE](LICENSE).

## Contributing

Issues and Pull Requests are welcome!

## Author

Benign X <1341398182@qq.com>