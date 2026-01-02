#!/bin/bash
# Installation script for {{BINARY_NAME}} v{{VERSION}}

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[✓]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

# Configuration
BINARY_NAME="{{BINARY_NAME}}"
VERSION="{{VERSION}}"
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="/etc/${BINARY_NAME}"
DATA_DIR="/var/lib/${BINARY_NAME}"
LOG_DIR="/var/log/${BINARY_NAME}"
SERVICE_NAME="${BINARY_NAME}"
INSTALL_MODE="native"  # Default to native mode
LOAD_SEED_DATA=""       # Ask user by default
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PACKAGE_DIR="$(dirname "$SCRIPT_DIR")"

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    log_error "This script must be run as root"
    exit 1
fi

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --native)
            INSTALL_MODE="native"
            shift
            ;;
        --docker)
            INSTALL_MODE="docker"
            shift
            ;;
        --seed-data)
            LOAD_SEED_DATA="yes"
            shift
            ;;
        --no-seed-data)
            LOAD_SEED_DATA="no"
            shift
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --native         Install in native mode (systemd service)"
            echo "  --docker         Install in Docker mode (runs in package directory)"
            echo "  --seed-data      Load seed data (demo users, sample assets)"
            echo "  --no-seed-data   Skip loading seed data"
            echo "  --help, -h       Show this help message"
            echo ""
            echo "Examples:"
            echo "  $0 --native --seed-data    # Native mode with seed data"
            echo "  $0 --docker --no-seed-data # Docker mode without seed data"
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            echo "Use --help to see available options"
            exit 1
            ;;
    esac
done

log_info "Installing ${BINARY_NAME} v${VERSION}"
log_info "Installation mode: ${INSTALL_MODE}"
echo ""

# Ask about seed data if not specified
if [ -z "$LOAD_SEED_DATA" ]; then
    echo ""
    log_info "Do you want to load seed data?"
    echo "  Seed data includes:"
    echo "    - Demo user account (demo/Demo123!)"
    echo "    - Test user accounts (john.doe, jane.smith, bob.wilson)"
    echo "    - Sample asset groups (dev/stage/prod)"
    echo "    - Sample hosts (8 example hosts)"
    echo "    - Sample audit logs and login events"
    echo ""
    echo -n "Load seed data? [y/N] "
    read -r response
    if [[ "$response" =~ ^[Yy]$ ]]; then
        LOAD_SEED_DATA="yes"
        log_info "Seed data will be loaded"
    else
        LOAD_SEED_DATA="no"
        log_info "Seed data will NOT be loaded"
    fi
    echo ""
fi

# Step 1: Create user (only for native mode)
if [ "$INSTALL_MODE" = "native" ]; then
    log_info "Step 1: Creating system user"
    if ! id "$BINARY_NAME" &>/dev/null; then
        useradd -r -s /bin/false -d "$DATA_DIR" "$BINARY_NAME"
        log_success "Created user: $BINARY_NAME"
    else
        log_warn "User $BINARY_NAME already exists"
    fi
fi

# Step 2: Create directories (only for native mode)
if [ "$INSTALL_MODE" = "native" ]; then
    log_info "Step 2: Creating directories"
    mkdir -p "$INSTALL_DIR"
    mkdir -p "$CONFIG_DIR"
    mkdir -p "$DATA_DIR/migrations"
    mkdir -p "$LOG_DIR"
    chown -R "${BINARY_NAME}:${BINARY_NAME}" "$DATA_DIR" "$LOG_DIR"
    chmod 755 "$CONFIG_DIR" "$DATA_DIR" "$LOG_DIR"
    log_success "Created directories"
fi

# Step 3: Install binary (only for native mode)
if [ "$INSTALL_MODE" = "native" ]; then
    log_info "Step 3: Installing binary"
    cp "$PACKAGE_DIR/bin/${BINARY_NAME}" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/${BINARY_NAME}"
    chown root:root "$INSTALL_DIR/${BINARY_NAME}"
    log_success "Installed binary to $INSTALL_DIR"
fi

# Step 4: Install migrations (only for native mode)
if [ "$INSTALL_MODE" = "native" ]; then
    log_info "Step 4: Installing migrations"
    if [ -d "$PACKAGE_DIR/migrations" ]; then
        cp -r "$PACKAGE_DIR/migrations/"* "$DATA_DIR/migrations/"
        chown -R "${BINARY_NAME}:${BINARY_NAME}" "$DATA_DIR/migrations"
        log_success "Installed migrations"
    fi
fi

# Step 5: Mode-specific installation
if [ "$INSTALL_MODE" = "docker" ]; then
    # Docker mode installation - everything stays in package directory
    log_info "Step 5: Docker mode setup"
    echo ""

    # Check if Docker is installed
    if ! command -v docker &> /dev/null; then
        log_error "Docker is not installed"
        log_error "Please install Docker first:"
        log_error "  curl -fsSL https://get.docker.com | sh"
        exit 1
    fi

    # Check if Docker Compose is installed
    if ! command -v docker-compose &> /dev/null && ! docker compose version &> /dev/null; then
        log_error "Docker Compose is not installed"
        log_error "Please install Docker Compose first"
        exit 1
    fi
    log_success "Docker and Docker Compose are installed"

    # Docker files are already in package directory
    DOCKER_DIR="$PACKAGE_DIR/docker"

    # Verify docker-compose.yml exists
    if [ ! -f "$DOCKER_DIR/docker-compose.yml" ]; then
        log_error "Docker Compose configuration not found: $DOCKER_DIR/docker-compose.yml"
        exit 1
    fi
    log_success "Docker configuration found in package directory"

    # Create .env file for Docker (in package directory)
    if [ ! -f "$DOCKER_DIR/.env" ]; then
        log_info "Creating Docker environment file..."
        cat > "$DOCKER_DIR/.env" <<DOCKEREOF
# PostgreSQL Configuration
POSTGRES_DB=ops_system
POSTGRES_USER=ops_user
POSTGRES_PASSWORD=$(openssl rand -base64 32 | tr -d "=+/" | cut -c1-25)

# Application Configuration (NOTE: Double underscore __ for nested fields)
LOG_LEVEL=info
ALLOWED_IPS=

# Seed Data Configuration
LOAD_SEED_DATA=${LOAD_SEED_DATA}

# OPS Application Environment Variables
# Server Configuration
OPS_SERVER__ADDR=0.0.0.0:3000
OPS_SERVER__GRACEFUL_SHUTDOWN_TIMEOUT_SECS=30

# Database Configuration
OPS_DATABASE__URL=postgresql://ops_user:\${POSTGRES_PASSWORD}@postgres:5432/\${POSTGRES_DB}
OPS_DATABASE__MAX_CONNECTIONS=10
OPS_DATABASE__MIN_CONNECTIONS=2
OPS_DATABASE__ACQUIRE_TIMEOUT_SECS=30
OPS_DATABASE__IDLE_TIMEOUT_SECS=600
OPS_DATABASE__MAX_LIFETIME_SECS=1800

# Logging Configuration
OPS_LOGGING__LEVEL=\${LOG_LEVEL:-info}
OPS_LOGGING__FORMAT=json

# Security Configuration
OPS_SECURITY__JWT_SECRET=$(openssl rand -base64 48 | tr -d "=+/" | cut -c1-48)
OPS_SECURITY__ACCESS_TOKEN_EXP_SECS=900
OPS_SECURITY__REFRESH_TOKEN_EXP_SECS=604800
OPS_SECURITY__PASSWORD_MIN_LENGTH=8
OPS_SECURITY__PASSWORD_REQUIRE_UPPERCASE=true
OPS_SECURITY__PASSWORD_REQUIRE_DIGIT=true
OPS_SECURITY__PASSWORD_REQUIRE_SPECIAL=false
OPS_SECURITY__MAX_LOGIN_ATTEMPTS=5
OPS_SECURITY__LOGIN_LOCKOUT_DURATION_SECS=1800
OPS_SECURITY__RATE_LIMIT_RPS=100
OPS_SECURITY__TRUST_PROXY=true
DOCKEREOF
        log_success "Docker environment file created: $DOCKER_DIR/.env"
    else
        # Update existing .env file with LOAD_SEED_DATA
        if grep -q "^LOAD_SEED_DATA=" "$DOCKER_DIR/.env"; then
            sed -i "s/^LOAD_SEED_DATA=.*/LOAD_SEED_DATA=${LOAD_SEED_DATA}/" "$DOCKER_DIR/.env"
        else
            echo "LOAD_SEED_DATA=${LOAD_SEED_DATA}" >> "$DOCKER_DIR/.env"
        fi
        log_success "Docker environment file updated: $DOCKER_DIR/.env"
    fi

    # Create marker file to indicate Docker mode installation
    echo "$VERSION" > "$PACKAGE_DIR/.docker-mode"
    log_success "Docker mode marker created"

    # Summary for Docker mode
    echo ""
    log_success "========================================="
    log_success "Docker Installation completed!"
    log_success "========================================="
    echo ""
    echo "Configuration:"
    if [ "$LOAD_SEED_DATA" = "yes" ]; then
        echo "  Seed data: ✓ Enabled"
        echo "    - Demo accounts will be created"
        echo "    - Sample assets will be loaded"
    else
        echo "  Seed data: ✗ Disabled (clean installation)"
    fi
    echo ""
    echo "All files are in the package directory: $PACKAGE_DIR"
    echo ""
    echo "Quick start:"
    echo "  1. Review configuration: cat $DOCKER_DIR/.env"
    echo "  2. Start services: cd $DOCKER_DIR && docker-compose up -d"
    echo "  3. View logs: docker-compose logs -f"
    echo "  4. Stop services: docker-compose down"
    echo ""
    echo "Services deployed:"
    echo "  - PostgreSQL database (port 5432, localhost only)"
    echo "  - API service (internal, accessible via Nginx)"
    echo "  - Nginx reverse proxy (ports 80, 443)"
    echo ""
    if [ "$LOAD_SEED_DATA" = "yes" ]; then
        echo "Default accounts after first start:"
        echo "  - admin / Admin123! (Administrator)"
        echo "  - demo  / Demo123!  (Operator)"
        echo ""
        log_warn "Remember to change default passwords!"
    fi
    echo ""
    log_info "To start the services now, run:"
    log_info "  cd $DOCKER_DIR"
    log_info "  docker-compose up -d"
    echo ""
    echo "For more information, see the documentation in the docs/ directory."
    exit 0
fi

# Native mode installation
log_info "Step 5: Setting up configuration (native mode)"
if [ ! -f "$CONFIG_DIR/env" ]; then
    if [ -f "$PACKAGE_DIR/config/.env.example" ]; then
        cp "$PACKAGE_DIR/config/.env.example" "$CONFIG_DIR/env"
        log_warn "Configuration file created: $CONFIG_DIR/env"
        log_warn "Please edit this file with your settings before starting the service"
    else
        cat > "$CONFIG_DIR/env" <<'ENVEOF'
# Database configuration (NOTE: Use double underscore __ for nested fields)
OPS_DATABASE__URL=postgresql://postgres:changeme@127.0.0.1:5432/ops_system
OPS_DATABASE__MAX_CONNECTIONS=10
OPS_DATABASE__MIN_CONNECTIONS=2

# Server configuration
OPS_SERVER__ADDR=0.0.0.0:3000

# Security
OPS_SECURITY__JWT_SECRET=change-this-to-a-random-min-32-char-string
OPS_SECURITY__RATE_LIMIT_RPS=100

# Logging
OPS_LOGGING__LEVEL=info
ENVEOF
        log_warn "Default configuration created: $CONFIG_DIR/env"
        log_warn "Please edit this file with your settings"
        log_warn "IMPORTANT: Use double underscore (__) for nested configuration fields"
    fi
    chown "${BINARY_NAME}:${BINARY_NAME}" "$CONFIG_DIR/env"
    chmod 640 "$CONFIG_DIR/env"
else
    log_warn "Configuration file already exists, skipping"
fi

# Step 6: Install systemd service (only for native mode)
log_info "Step 6: Installing systemd service"
if [ -f "$PACKAGE_DIR/systemd/${BINARY_NAME}.service" ]; then
    cp "$PACKAGE_DIR/systemd/${BINARY_NAME}.service" /etc/systemd/system/
    systemctl daemon-reload
    log_success "Installed systemd service"
else
    log_warn "Systemd service file not found, skipping"
fi

# Step 7: Database setup (native mode)
log_info "Step 7: Database setup (native mode)"
echo ""

# Check if PostgreSQL is installed
if ! command -v psql &> /dev/null; then
    log_warn "PostgreSQL client not found. Installing..."
    if command -v apt-get &> /dev/null; then
        apt-get update -qq
        apt-get install -y postgresql-client
    elif command -v yum &> /dev/null; then
        yum install -y postgresql
    elif command -v pacman &> /dev/null; then
        pacman -S --noconfirm postgresql
    else
        log_warn "Could not install PostgreSQL client automatically"
        log_warn "Please install it manually and re-run this script"
    fi
fi

# Extract database URL from config
if [ -f "$CONFIG_DIR/env" ]; then
    DB_URL=$(grep "^OPS_DATABASE__URL=" "$CONFIG_DIR/env" | cut -d'=' -f2-)

    if [ -n "$DB_URL" ]; then
        # Parse database name
        DB_NAME=$(echo "$DB_URL" | sed -n 's|.*/\(.*\)|\1|p')

        log_info "Checking if database '$DB_NAME' exists..."

        # Check if PostgreSQL is running
        if ! systemctl is-active --quiet postgresql 2>/dev/null; then
            log_warn "PostgreSQL is not running. Starting it..."
            systemctl start postgresql
            sleep 2
        fi

        # Try to create database if it doesn't exist
        if command -v psql &> /dev/null; then
            # Check if database exists
            DB_EXISTS=$(sudo -u postgres psql -tAc "SELECT 1 FROM pg_database WHERE datname='$DB_NAME'" 2>/dev/null)

            if [ "$DB_EXISTS" != "1" ]; then
                log_warn "Database '$DB_NAME' does not exist. Creating..."
                if sudo -u postgres psql -c "CREATE DATABASE $DB_NAME;" &> /dev/null; then
                    log_success "Database '$DB_NAME' created successfully"
                else
                    log_warn "Failed to create database automatically"
                    log_warn "You may need to create it manually:"
                    log_warn "  sudo -u postgres psql"
                    log_warn "  CREATE DATABASE $DB_NAME;"
                    log_warn "  \\q"
                fi
            else
                log_success "Database '$DB_NAME' already exists"
            fi
        fi

        # Test database connection
        log_info "Testing database connection..."
        if timeout 5 sudo -u postgres psql "$DB_URL" -c "SELECT 1" &> /dev/null 2>&1; then
            log_success "Database connection test passed"
        else
            log_warn "Could not test database connection with postgres user"
            log_warn "This is normal if your database uses a different user"
        fi
    fi
fi

# Step 8: Load seed data (Native mode)
if [ "$LOAD_SEED_DATA" = "yes" ]; then
    log_info "Step 8: Loading seed data..."
    echo ""

    if [ -f "$PACKAGE_DIR/migrations/000003_seed_data.sql" ]; then
        log_info "Running seed data script..."
        if sudo -u postgres psql "$DB_URL" -f "$PACKAGE_DIR/migrations/000003_seed_data.sql" &> /dev/null; then
            log_success "Seed data loaded successfully"
        else
            log_warn "Failed to load seed data"
            log_warn "You can load it manually later:"
            log_warn "  psql -U postgres -d $DB_NAME -f migrations/000003_seed_data.sql"
        fi
    else
        log_warn "Seed data file not found"
    fi
    echo ""
fi

# Step 9: Summary (Native mode)
echo ""
log_success "========================================="
log_success "Native Installation completed!"
log_success "========================================="
echo ""
echo "Configuration:"
if [ "$LOAD_SEED_DATA" = "yes" ]; then
    echo "  Seed data: ✓ Loaded"
    echo "    - Demo accounts created"
    echo "    - Sample assets loaded"
else
    echo "  Seed data: ✗ Not loaded (clean installation)"
fi
echo ""
echo "Quick start:"
echo "  1. (Optional) Edit configuration: nano $CONFIG_DIR/env"
echo "  2. Start service: ./scripts/start.sh"
echo "  3. Check status: systemctl status ${SERVICE_NAME}"
echo ""
echo "Service will automatically:"
echo "  - Run database migrations on first start"
echo "  - Create necessary tables"
echo "  - Start listening on configured port"
echo ""
if [ "$LOAD_SEED_DATA" = "yes" ]; then
    echo "Default accounts:"
    echo "  - admin / Admin123! (Administrator)"
    echo "  - demo  / Demo123!  (Operator)"
    echo ""
    log_warn "Remember to change default passwords!"
fi
echo ""
echo "For more information, see the documentation in the docs/ directory."
