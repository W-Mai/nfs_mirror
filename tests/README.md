# Test Configuration Files

This directory contains various configuration files and scripts for testing the NFS Mirror project.

## File Descriptions

### Configuration Files

#### `invalid_config.toml`
Invalid configuration file example for testing error handling:
- Contains invalid IP address format
- Used to verify that the program can correctly detect and report configuration errors

#### `nonexistent_config.toml`
Configuration file with non-existent paths for testing:
- Error handling when source directory does not exist
- Path validation functionality

#### `sample_config.toml`
Complete configuration file example showing all available configuration options:
- Server configuration (IP, port, log level, etc.)
- Multiple mount point configurations
- Various mount options (read-only, descriptions, etc.)

#### `test_config.toml`
Basic test configuration file for:
- Single mount point basic functionality testing
- Default configuration validation

#### `test_multi_mount.toml`
Multi-mount point test configuration file for:
- Testing multiple directories mounted simultaneously
- Complex configuration scenario validation

### Test Scripts

#### `test_nfs.sh`
NFS functionality test script including:
- NFS server connection testing
- Mount/unmount functionality testing
- File read/write operation testing

## Usage

These configuration files are primarily used for:

1. **Development Testing**: Verify configuration parsing and error handling logic
2. **Example Reference**: Serve as reference for users writing configuration files
3. **CI/CD Testing**: Use in automated testing

### Running Tests

The `test.sh` script in the project root automatically uses these configuration files for functionality testing:

```bash
./test.sh
```

Run specific NFS functionality tests:

```bash
./tests/test_nfs.sh
```

## Notes

- These configuration files are for testing only, do not use in production environments
- Test scripts create temporary configuration files and automatically clean up after testing
- Some configuration files contain intentional error configurations for testing error handling mechanisms
- Temporary test files are ignored by `.gitignore` and will not be committed to version control