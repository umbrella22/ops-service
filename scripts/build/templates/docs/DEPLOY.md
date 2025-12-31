# Deployment Guide for {{BINARY_NAME}} v{{VERSION}}

## Table of Contents
- [System Requirements](#system-requirements)
- [Quick Start](#quick-start)
- [Installation](#installation)
- [Configuration](#configuration)
- [Database Setup](#database-setup)
- [Starting the Service](#starting-the-service)
- [Verification](#verification)
- [Docker Deployment](#docker-deployment)
- [Troubleshooting](#troubleshooting)

## System Requirements

### Minimum Requirements
- **OS**: Linux (x86_64 or ARM64)
- **RAM**: 512 MB minimum, 1 GB recommended
- **Disk**: 100 MB for binary + space for database
- **CPU**: 1 core minimum

### Software Dependencies
- PostgreSQL 12+
- systemd (for service management)

### Optional Dependencies
- Nginx (for reverse proxy)
- Docker (for containerized deployment)

## Quick Start

```bash
# 1. Extract the archive
tar -xzf {{BINARY_NAME}}-{{VERSION}}-linux-{{PLATFORM}}.tar.gz
cd linux-{{PLATFORM}}

# 2. Run the installer
sudo ./scripts/install.sh

# 3. Configure the database
sudo nano /etc/{{BINARY_NAME}}/env

# 4. Start the service
sudo systemctl start {{BINARY_NAME}}

# 5. Check status
sudo systemctl status {{BINARY_NAME}}
```

## Installation

### Method 1: Automated Installation (Recommended)

The included installer script will:
- Create a dedicated system user
- Install the binary to `/usr/local/bin`
- Set up required directories
- Install systemd service
- Create default configuration

```bash
cd linux-{{PLATFORM}}
sudo ./scripts/install.sh
```

### Method 2: Manual Installation

```bash
# 1. Create user
sudo useradd -r -s /bin/false -d /var/lib/{{BINARY_NAME}} {{BINARY_NAME}}

# 2. Create directories
sudo mkdir -p /usr/local/bin
sudo mkdir -p /etc/{{BINARY_NAME}}
sudo mkdir -p /var/lib/{{BINARY_NAME}}/migrations
sudo mkdir -p /var/log/{{BINARY_NAME}}

# 3. Install binary
sudo cp bin/{{BINARY_NAME}} /usr/local/bin/
sudo chmod +x /usr/local/bin/{{BINARY_NAME}}

# 4. Install migrations
sudo cp -r migrations/* /var/lib/{{BINARY_NAME}}/migrations/
sudo chown -R {{BINARY_NAME}}:{{BINARY_NAME}} /var/lib/{{BINARY_NAME}}

# 5. Install systemd service
sudo cp systemd/{{BINARY_NAME}}.service /etc/systemd/system/
sudo systemctl daemon-reload
```

## Configuration

The main configuration file is located at `/etc/{{BINARY_NAME}}/env`.

### Required Settings

```bash
# Database URL (REQUIRED)
OPS_DATABASE_URL=postgresql://user:password@localhost:5432/dbname

# JWT Secret (REQUIRED, min 32 characters)
OPS_SECURITY_JWT_SECRET=your-random-secret-key-min-32-chars
```

### Optional Settings

```bash
# Server
OPS_SERVER_ADDR=0.0.0.0:3000
OPS_SERVER_GRACEFUL_SHUTDOWN_TIMEOUT_SECS=30

# Database
OPS_DATABASE_MAX_CONNECTIONS=10
OPS_DATABASE_MIN_CONNECTIONS=2

# Logging
OPS_LOGGING_LEVEL=info
OPS_LOGGING_FORMAT=json

# Security
OPS_SECURITY_RATE_LIMIT_RPS=100
OPS_SECURITY_TRUST_PROXY=true
```

After editing the configuration, restart the service:
```bash
sudo systemctl restart {{BINARY_NAME}}
```

## Database Setup

### PostgreSQL Installation

```bash
# Ubuntu/Debian
sudo apt install postgresql postgresql-contrib

# RHEL/CentOS
sudo yum install postgresql-server postgresql-contrib
sudo postgresql-setup initdb
sudo systemctl start postgresql
sudo systemctl enable postgresql
```

### Create Database and User

```bash
# Switch to postgres user
sudo -u postgres psql

# In psql:
CREATE DATABASE ops_system;
CREATE USER ops_user WITH ENCRYPTED PASSWORD 'your_password';
GRANT ALL PRIVILEGES ON DATABASE ops_system TO ops_user;
\q
```

### Run Migrations

```bash
# Install sqlx-cli if not already installed
cargo install sqlx-cli --no-default-features --features rustls,postgres

# Run migrations
export OPS_DATABASE_URL="postgresql://ops_user:your_password@localhost:5432/ops_system"
sqlx migrate run --database-url $OPS_DATABASE_URL --source-dir /var/lib/{{BINARY_NAME}}/migrations
```

## Starting the Service

### Start the service
```bash
sudo ./scripts/start.sh
# or
sudo systemctl start {{BINARY_NAME}}
```

### Enable at boot
```bash
sudo systemctl enable {{BINARY_NAME}}
```

### Check status
```bash
sudo ./scripts/status.sh
# or
sudo systemctl status {{BINARY_NAME}}
```

### View logs
```bash
sudo journalctl -u {{BINARY_NAME}} -f
```

## Verification

### Health Check

```bash
# Using curl
curl http://localhost:3000/health

# Expected output
{"status":"ok"}
```

### Ready Check

```bash
curl http://localhost:3000/ready

# Expected output if database is connected
{"status":"ready"}
```

### Check Logs

```bash
sudo journalctl -u {{BINARY_NAME}} -n 50
```

## Docker Deployment

### Using Docker Compose

```bash
cd docker/
docker compose up -d
```

### Manual Docker Build

```bash
cd docker/
docker build -t {{BINARY_NAME}}:{{VERSION}} .
docker run -d \
  --name {{BINARY_NAME}} \
  -p 3000:3000 \
  -e OPS_DATABASE_URL="postgresql://..." \
  -e OPS_SECURITY_JWT_SECRET="..." \
  {{BINARY_NAME}}:{{VERSION}}
```

### With Nginx Reverse Proxy

The package includes Nginx configuration files. Copy them to your Nginx server:

```bash
sudo cp nginx/nginx.conf /etc/nginx/nginx.conf
sudo cp nginx/ssl.conf /etc/nginx/conf.d/ssl.conf
sudo nginx -t
sudo systemctl reload nginx
```

## Troubleshooting

### Service Won't Start

1. Check logs:
   ```bash
   sudo journalctl -u {{BINARY_NAME}} -n 100
   ```

2. Verify configuration:
   ```bash
   sudo cat /etc/{{BINARY_NAME}}/env
   ```

3. Check database connection:
   ```bash
   psql "postgresql://user:pass@localhost:5432/dbname"
   ```

### Permission Issues

```bash
# Fix permissions
sudo chown -R {{BINARY_NAME}}:{{BINARY_NAME}} /var/lib/{{BINARY_NAME}}
sudo chown -R {{BINARY_NAME}}:{{BINARY_NAME}} /var/log/{{BINARY_NAME}}
sudo chmod 640 /etc/{{BINARY_NAME}}/env
```

### Database Connection Errors

1. Verify PostgreSQL is running:
   ```bash
   sudo systemctl status postgresql
   ```

2. Check connection string in `/etc/{{BINARY_NAME}}/env`

3. Ensure database exists and user has permissions

### Port Already in Use

Change the port in `/etc/{{BINARY_NAME}}/env`:
```bash
OPS_SERVER_ADDR=0.0.0.0:3001
```

### Get Help

- Check logs: `sudo journalctl -u {{BINARY_NAME}} -f`
- Review troubleshooting guide: [TROUBLESHOOTING.md](TROUBLESHOOTING.md)
- Check system resources: `htop` or `top`

## Next Steps

- Review the [upgrade guide](UPGRADE.md) for version updates
- Configure reverse proxy with Nginx
- Set up automated backups
- Configure monitoring and alerting
- Review security best practices
