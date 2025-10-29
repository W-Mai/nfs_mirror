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
- 🔌 Multi-mount point configuration
- 📁 Single directory and multi-directory modes
- 🔐 Read/write permission control (global and per-mount point)
- 📝 TOML configuration file support
- 🎮 Dynamic mount point management

## Installation

```bash
# Clone project
git clone <repository-url>
cd nfs_mirror

# Build release version
cargo build --release

# Binary location
./target/release/nfs_mirror

# Install via Cargo
cargo install --path .
```

## Usage

### 1. Single Directory Mode

```bash
# Basic usage
nfs_mirror /path/to/directory -t /mount_point

# With verbose logging
nfs_mirror /path/to/directory -t /mount_point -v

# Read-only mode
nfs_mirror /path/to/directory -t /mount_point --read-only

# Specify IP and port
nfs_mirror /path/to/directory --ip 0.0.0.0 --port 11451

# Enable verbose output
nfs_mirror /path/to/directory --verbose
```

### 2. Configuration File Mode

Create configuration file `config.toml`:

```toml
[server]
ip = "127.0.0.1"
port = 11451
log_level = "info"
verbose = true
read_only = false

[[mounts]]
source = "/Users/w-mai/Projects/Rust/nfs_mirror/src"
target = "/source"
read_only = false
description = "Source code directory"

[[mounts]]
source = "/tmp"
target = "/temp"
read_only = false
description = "Temporary files directory"
```

Start service:

```bash
nfs_mirror -c config.toml
```

### 3. Generate Example Configuration File

```bash
nfs_mirror --generate-config example.toml
```

### 4. Advanced Configuration

```bash
# Complete configuration example
nfs_mirror /path/to/directory \
    --ip 192.168.1.100 \
    --port 11451 \
    --log-level info \
    --verbose \
    --max-connections 200 \
    --read-timeout 60 \
    --write-timeout 60 \
    --read-only \
    --allow-ips "192.168.1.0/24,10.0.0.100"
```

### 5. Daemon Mode

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
- `-c, --config <CONFIG>`: Configuration file path
- `--generate-config <GENERATE_CONFIG>`: Generate example configuration file

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

### Mount Options Explanation

- `nolocks`: Disable file locks (recommended for local testing)
- `vers=3`: Use NFSv3 protocol
- `tcp`: Use TCP protocol
- `port=11451`: NFS port
- `mountport=11451`: Mount port
- `soft`: Soft mount (returns error after timeout)

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
    --port 11451 \
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
    --port 11451 \
    --max-connections 1000
```

## Logs and Monitoring

### Log Levels

- `error`: Error messages only
- `warn`: Warnings and errors
- `info`: General information (recommended)
- `debug`: Debug information
- `trace`: Detailed trace information

### Example Log Output

```
INFO  nfs_mirror::cli: NFS Mirror service starting...
INFO  nfs_mirror::cli: Listen address: 127.0.0.1:11451
INFO  nfs_mirror::cli: Configured mount points:
INFO  nfs_mirror::cli:   1: /Users/w-mai/Projects/Rust/nfs_mirror/src -> /source (read-only: No)
INFO  nfs_mirror::cli: NFS service started, waiting for client connections...
INFO  zerofs_nfsserve::tcp: Listening on 127.0.0.1:11451
```

## Error Handling

The program validates configuration and provides detailed error messages:

- Configuration file syntax errors
- IP address format errors
- Source directory does not exist
- Duplicate target paths
- Port configuration errors

## Performance Optimization

1. **Use appropriate log levels**: Production environments recommend `info` or `warn`
2. **Adjust timeout settings**: Adjust read/write timeouts based on network environment
3. **Connection limits**: Adjust maximum connections based on server performance
4. **Memory usage**: Large directories recommend increasing system memory

## Security Considerations

1. **Access Control**: Use `allow_ips` to restrict client IP access
2. **Read-only Mode**: Enable read-only mode for sensitive data
3. **Firewall**: Ensure firewall rules are properly configured
4. **User Permissions**: Ensure the mirrored directory permissions are set correctly
5. **Run as non-privileged user**: Run the service with minimal privileges

## Troubleshooting

### Common Issues

1. **Mount Failure**
    - Check network connection
    - Verify port is not occupied
    - Confirm firewall settings

2. **Permission Errors**
    - Check source directory permissions
    - Verify NFS client permissions

3. **Performance Issues**
    - Adjust timeout settings
    - Check network latency
    - Monitor system resources

### Debug Commands

```bash
# Enable verbose logging
nfs_mirror -c config.toml -l debug -v

# Check port usage
netstat -an | grep 11451

# Test with single directory
nfs_mirror /path/to/directory -t /mount_point -v
```

## Testing

The project includes comprehensive test functionality:

### Manual Testing

```bash
# Create test directory
mkdir -p /tmp/nfs_test
echo "Hello NFS" > /tmp/nfs_test/test.txt

# Start server (in one terminal)
nfs_mirror /tmp/nfs_test -t /test -v

# Mount in another terminal
sudo mkdir -p /mnt/nfs_test
sudo mount -t nfs -o nolocks,vers=3,tcp,port=11451,mountport=11451,soft 127.0.0.1:/test /mnt/nfs_test

# Test file access
ls -la /mnt/nfs_test/
cat /mnt/nfs_test/test.txt

# Cleanup
sudo umount /mnt/nfs_test
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.