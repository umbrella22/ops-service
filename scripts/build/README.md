# Build Scripts Documentation

This directory contains the build and packaging system for the ops-system project.

## Directory Structure

```
scripts/build/
├── common.sh              # Shared utility functions
├── build.sh               # Binary compilation script
├── package.sh             # Main packaging orchestration
├── dist.sh                # Distribution archive creation
├── validate.sh            # Package validation
├── generate_scripts.sh    # Script generation from templates
└── templates/             # Template files for generated scripts
    ├── *.sh              # Management script templates
    ├── systemd.service   # Systemd service template
    └── docs/             # Documentation templates
        ├── DEPLOY.md
        ├── UPGRADE.md
        └── TROUBLESHOOTING.md
```

## Usage

### Quick Start

```bash
# Build for current platform (x86_64)
make package

# Build for all platforms
make package-all

# Create distribution archives
make dist-all
```

### Cross-Compilation Setup

For ARM64 builds, install the cross-compilation toolchain:

```bash
# Ubuntu/Debian
sudo apt install gcc-aarch64-linux-gnu

# Add Rust target
rustup target add aarch64-unknown-linux-gnu

# Build ARM64 package
make package-arm64
```

## Build Scripts

### common.sh
Shared utilities used by all build scripts:
- Logging functions with colors
- Version extraction from Cargo.toml
- Checksum calculation
- Directory creation helpers
- File operations

### build.sh
Compiles the binary for a specific platform:
- Supports x86_64 (native) and arm64 (cross-compile)
- Validates toolchain installation
- Verifies binary output

### package.sh
Main orchestration script that:
1. Calls build.sh to compile binary
2. Creates directory structure
3. Copies binaries, migrations, configs
4. Generates management scripts
5. Creates systemd service file
6. Generates documentation
7. Calculates checksums
8. Creates metadata files

### dist.sh
Creates distribution archives:
- Packages build directory into tar.gz
- Generates SHA256 checksums
- Creates versioned archives

### validate.sh
Validates package contents:
- Checks all required files exist
- Verifies permissions
- Validates checksums
- Tests binary execution

## Templates

Templates contain placeholders that are substituted during build:

- `{{BINARY_NAME}}` - Replaced with actual binary name from Cargo.toml
- `{{VERSION}}` - Replaced with version from Cargo.toml
- `{{PLATFORM}}` - Replaced with target platform (x86_64 or arm64)

### Management Scripts

Each script template is processed during package creation:
- install.sh - Automated installation
- start.sh - Start systemd service
- stop.sh - Stop systemd service
- restart.sh - Restart systemd service
- status.sh - Show service status and logs
- update.sh - Update to new version
- backup.sh - Backup configuration and data
- uninstall.sh - Remove installation

### Documentation

Generated documentation includes:
- DEPLOY.md - Deployment guide
- UPGRADE.md - Version upgrade procedures
- TROUBLESHOOTING.md - Common issues and solutions

## Build Output

Packages are created in the `build/` directory:

```
build/
├── linux-x86_64/          # Platform-specific directory
│   ├── bin/              # Compiled binary
│   ├── migrations/       # Database migrations
│   ├── config/           # Configuration templates
│   ├── docker/           # Docker files
│   ├── nginx/            # Nginx configuration
│   ├── scripts/          # Generated management scripts
│   ├── systemd/          # Systemd service file
│   ├── docs/             # Generated documentation
│   ├── VERSION           # Version file
│   ├── CHECKSUM          # SHA256 checksums
│   └── BUILD_INFO.txt    # Build metadata
└── dist/                 # Distribution archives
    └── ops-system-{version}-linux-{platform}.tar.gz
```

## Environment Variables

The build system respects these environment variables:

- `OPS_DATABASE_URL` - Database connection string
- `RUST_LOG` - Logging level for build output
- `CARGO_BUILD_TARGET` - Override default target triple

## Error Handling

All scripts use `set -euo pipefail` for strict error handling:
- `e` - Exit on error
- `u` - Exit on undefined variable
- `o pipefail` - Exit on pipe failure

## Contributing

When adding new features:

1. Add shared functions to `common.sh`
2. Create new templates in `templates/`
3. Update `package.sh` to include new files
4. Update `validate.sh` to check new files
5. Update this README

## Troubleshooting

### Cross-compilation fails

Ensure ARM64 toolchain is installed:
```bash
sudo apt install gcc-aarch64-linux-gnu
rustup target add aarch64-unknown-linux-gnu
```

### Permission denied on scripts

Scripts are made executable during generation. If issues persist:
```bash
chmod +x scripts/build/*.sh
```

### Build directory conflicts

Clean and rebuild:
```bash
make package-clean
make package
```

## See Also

- [Makefile](../../Makefile) - Build targets
- [.cargo/config.toml](../.cargo/config.toml) - Cargo configuration
- [README.md](../../README.md) - Project documentation
