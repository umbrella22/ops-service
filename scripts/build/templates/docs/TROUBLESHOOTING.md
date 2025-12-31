# Troubleshooting Guide for {{BINARY_NAME}}

## Table of Contents
- [Installation Issues](#installation-issues)
- [Service Startup Problems](#service-startup-problems)
- [Database Connection Issues](#database-connection-issues)
- [Performance Issues](#performance-issues)
- [Authentication Problems](#authentication-problems)
- [Network Issues](#network-issues)
- [Logging and Debugging](#logging-and-debugging)
- [Common Errors](#common-errors)

## Installation Issues

### Binary Not Found After Installation

**Symptoms**: `command not found` error when running {{BINARY_NAME}}

**Solutions**:
```bash
# Check if binary exists
ls -l /usr/local/bin/{{BINARY_NAME}}

# Check PATH
echo $PATH

# If binary exists but not in PATH, use full path
/usr/local/bin/{{BINARY_NAME}} --version

# Add to PATH if needed (add to ~/.bashrc or ~/.zshrc)
export PATH="/usr/local/bin:$PATH"
```

### Permission Denied Errors

**Symptoms**: `Permission denied` when running or accessing files

**Solutions**:
```bash
# Fix binary permissions
sudo chmod +x /usr/local/bin/{{BINARY_NAME}}

# Fix data directory permissions
sudo chown -R {{BINARY_NAME}}:{{BINARY_NAME}} /var/lib/{{BINARY_NAME}}
sudo chown -R {{BINARY_NAME}}:{{BINARY_NAME}} /var/log/{{BINARY_NAME}}

# Fix config file permissions
sudo chmod 640 /etc/{{BINARY_NAME}}/env
sudo chown {{BINARY_NAME}}:{{BINARY_NAME}} /etc/{{BINARY_NAME}}/env
```

### systemd Service Not Found

**Symptoms**: `Failed to enable unit: Unit file {{BINARY_NAME}}.service does not exist`

**Solutions**:
```bash
# Check if service file exists
ls -l /etc/systemd/system/{{BINARY_NAME}}.service

# Reload systemd daemon
sudo systemctl daemon-reload

# Reinstall service file
sudo cp /path/to/package/systemd/{{BINARY_NAME}}.service /etc/systemd/system/
sudo systemctl daemon-reload
```

## Service Startup Problems

### Service Fails to Start

**Symptoms**: `systemctl start {{BINARY_NAME}}` fails

**Debug Steps**:
```bash
# Check service status
sudo systemctl status {{BINARY_NAME}}

# View detailed logs
sudo journalctl -u {{BINARY_NAME}} -n 100 --no-pager

# Try starting manually
sudo -u {{BINARY_NAME}} /usr/local/bin/{{BINARY_NAME}}
```

**Common Causes**:

1. **Configuration file missing or invalid**:
   ```bash
   # Check config exists
   ls -l /etc/{{BINARY_NAME}}/env

   # Validate syntax
   cat /etc/{{BINARY_NAME}}/env
   ```

2. **Database connection failed**:
   - Verify PostgreSQL is running
   - Check connection string
   - Test database connectivity

3. **Port already in use**:
   ```bash
   # Check what's using port 3000
   sudo lsof -i :3000
   sudo netstat -tulpn | grep 3000

   # Change port in config
   OPS_SERVER_ADDR=0.0.0.0:3001
   ```

4. **Insufficient permissions**:
   ```bash
   # Fix ownership
   sudo chown -R {{BINARY_NAME}}:{{BINARY_NAME}} /var/lib/{{BINARY_NAME}}
   ```

### Service Crashes Immediately

**Symptoms**: Service starts then stops immediately

**Debug Steps**:
```bash
# Enable more verbose logging
echo "RUST_LOG=debug" | sudo tee -a /etc/{{BINARY_NAME}}/env
sudo systemctl restart {{BINARY_NAME}}

# Check logs
sudo journalctl -u {{BINARY_NAME}} -f
```

**Common Causes**:
- Invalid JWT secret (must be min 32 characters)
- Database not accessible
- Missing migrations
- Invalid configuration values

## Database Connection Issues

### Cannot Connect to PostgreSQL

**Symptoms**: `connection refused` or `could not connect to server` errors

**Solutions**:

1. **Check PostgreSQL is running**:
   ```bash
   sudo systemctl status postgresql
   sudo systemctl start postgresql
   ```

2. **Verify database exists**:
   ```bash
   sudo -u postgres psql -l
   ```

3. **Test connection manually**:
   ```bash
   psql "postgresql://user:pass@localhost:5432/dbname"
   ```

4. **Check connection string**:
   ```bash
   sudo cat /etc/{{BINARY_NAME}}/env | grep OPS_DATABASE_URL
   ```

5. **Check PostgreSQL configuration**:
   ```bash
   # Check if PostgreSQL listens on the right interface
   sudo cat /etc/postgresql/*/main/postgresql.conf | grep listen_addresses

   # Check pg_hba.conf for authentication
   sudo cat /etc/postgresql/*/main/pg_hba.conf
   ```

### Migration Failures

**Symptoms**: `sqlx migrate run` fails

**Solutions**:

1. **Check migration files exist**:
   ```bash
   ls -l /var/lib/{{BINARY_NAME}}/migrations/
   ```

2. **Run migrations manually**:
   ```bash
   export OPS_DATABASE_URL="postgresql://user:pass@localhost:5432/dbname"
   sqlx migrate run --database-url $OPS_DATABASE_URL
   ```

3. **Check migration table**:
   ```bash
   psql "postgresql://user:pass@localhost:5432/dbname" -c "SELECT * FROM _sqlx_migrations;"
   ```

### Connection Pool Exhausted

**Symptoms**: Application hangs or returns "connection pool exhausted"

**Solutions**:

1. **Increase pool size**:
   ```bash
   # In /etc/{{BINARY_NAME}}/env
   OPS_DATABASE_MAX_CONNECTIONS=20
   ```

2. **Check for connection leaks**:
   ```bash
   # Check active connections in PostgreSQL
   psql -c "SELECT count(*) FROM pg_stat_activity;"
   ```

3. **Reduce connection timeout**:
   ```bash
   OPS_DATABASE_ACQUIRE_TIMEOUT_SECS=10
   ```

## Performance Issues

### High Memory Usage

**Diagnosis**:
```bash
# Check memory usage
systemctl show {{BINARY_NAME}} -p MemoryCurrent
ps aux | grep {{BINARY_NAME}}

# Check memory over time
watch -n 1 'ps aux | grep {{BINARY_NAME}}'
```

**Solutions**:
1. Reduce database connection pool
2. Enable log rotation
3. Check for memory leaks (monitor over time)
4. Add resource limits in systemd

### High CPU Usage

**Diagnosis**:
```bash
# Check CPU usage
top -p $(pgrep {{BINARY_NAME}})

# Check thread usage
ps -eLf | grep {{BINARY_NAME}}
```

**Solutions**:
1. Check for excessive logging
2. Review database queries
3. Check for infinite loops in custom code
4. Enable CPU profiling

### Slow Response Times

**Diagnosis**:
```bash
# Check response time
time curl http://localhost:3000/health

# Check database query performance
sudo -u postgres psql -c "SELECT * FROM pg_stat_statements ORDER BY mean_exec_time DESC LIMIT 10;"
```

**Solutions**:
1. Add database indexes
2. Enable query caching
3. Increase connection pool
4. Use connection pooling (PgBouncer)

## Authentication Problems

### JWT Token Issues

**Symptoms**: `Invalid token` or `Token expired` errors

**Solutions**:

1. **Check JWT secret**:
   ```bash
   sudo cat /etc/{{BINARY_NAME}}/env | grep JWT_SECRET
   # Must be at least 32 characters
   ```

2. **Adjust token expiration**:
   ```bash
   OPS_SECURITY_ACCESS_TOKEN_EXP_SECS=900
   OPS_SECURITY_REFRESH_TOKEN_EXP_SECS=604800
   ```

3. **Verify token format**:
   - Check Authorization header format: `Bearer <token>`
   - Ensure token hasn't been tampered with

### Login Failures

**Symptoms**: Cannot authenticate users

**Solutions**:

1. **Check user exists in database**:
   ```bash
   psql "postgresql://user:pass@localhost/dbname" -c "SELECT * FROM users;"
   ```

2. **Verify password hashing works**:
   - Check Argon2 parameters
   - Review password complexity requirements

3. **Check rate limiting**:
   ```bash
   # Temporarily disable for testing
   OPS_SECURITY_RATE_LIMIT_RPS=1000
   ```

## Network Issues

### Cannot Access from External Host

**Symptoms**: Service works locally but not from other machines

**Solutions**:

1. **Check binding address**:
   ```bash
   # In /etc/{{BINARY_NAME}}/env
   OPS_SERVER_ADDR=0.0.0.0:3000  # Bind to all interfaces
   ```

2. **Check firewall**:
   ```bash
   # Ubuntu/Debian
   sudo ufw status
   sudo ufw allow 3000/tcp

   # RHEL/CentOS
   sudo firewall-cmd --list-all
   sudo firewall-cmd --add-port=3000/tcp --permanent
   sudo firewall-cmd --reload
   ```

3. **Check if service is listening**:
   ```bash
   sudo ss -tulpn | grep 3000
   sudo netstat -tulpn | grep 3000
   ```

### Behind Reverse Proxy Issues

**Symptoms**: Errors when accessing through Nginx/Apache

**Solutions**:

1. **Configure proxy to pass original IP**:
   ```bash
   OPS_SECURITY_TRUST_PROXY=true
   ```

2. **Check Nginx configuration**:
   ```nginx
   location / {
       proxy_pass http://localhost:3000;
       proxy_set_header Host $host;
       proxy_set_header X-Real-IP $remote_addr;
       proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
       proxy_set_header X-Forwarded-Proto $scheme;
   }
   ```

## Logging and Debugging

### Enable Debug Logging

**Temporary**:
```bash
sudo systemctl stop {{BINARY_NAME}}
sudo -u {{BINARY_NAME}} OPS_LOGGING_LEVEL=debug /usr/local/bin/{{BINARY_NAME}}
```

**Permanent**:
```bash
# In /etc/{{BINARY_NAME}}/env
OPS_LOGGING_LEVEL=debug
OPS_LOGGING_FORMAT=pretty  # More readable than json
sudo systemctl restart {{BINARY_NAME}}
```

### View Logs

**Real-time**:
```bash
sudo journalctl -u {{BINARY_NAME}} -f
```

**Last 100 lines**:
```bash
sudo journalctl -u {{BINARY_NAME}} -n 100
```

**Since last boot**:
```bash
sudo journalctl -u {{BINARY_NAME}} -b
```

**Export logs**:
```bash
sudo journalctl -u {{BINARY_NAME}} --since "1 hour ago" > {{BINARY_NAME}}-logs.txt
```

### Check Application Logs

If logging to file:
```bash
sudo tail -f /var/log/{{BINARY_NAME}}/*.log
```

## Common Errors

### "Address already in use"
```bash
# Find process using port
sudo lsof -i :3000

# Kill process (if needed)
sudo kill <PID>

# Or change port in config
OPS_SERVER_ADDR=0.0.0.0:3001
```

### "Permission denied" on log file
```bash
sudo chown {{BINARY_NAME}}:{{BINARY_NAME}} /var/log/{{BINARY_NAME}}
sudo chmod 755 /var/log/{{BINARY_NAME}}
```

### "Database locked" or "Database is locked"
```bash
# Check for other connections
psql -c "SELECT * FROM pg_stat_activity WHERE datname='ops_system';"

# Terminate stale connections
psql -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname='ops_system' AND pid <> pg_backend_pid();"
```

### "Out of memory"
```bash
# Add memory limit to systemd
sudo systemctl edit {{BINARY_NAME}}

# Add:
# [Service]
# MemoryLimit=512M
```

## Getting Help

If you can't resolve the issue:

1. **Collect diagnostic information**:
   ```bash
   # Version
   /usr/local/bin/{{BINARY_NAME}} --version

   # Configuration
   sudo cat /etc/{{BINARY_NAME}}/env | sed 's/PASSWORD=.*/PASSWORD=***/'

   # Logs
   sudo journalctl -u {{BINARY_NAME}} -n 200 > {{BINARY_NAME}}-logs.txt

   # System info
   uname -a
   free -h
   df -h
   ```

2. **Review documentation**:
   - [DEPLOY.md](DEPLOY.md)
   - [UPGRADE.md](UPGRADE.md)

3. **Search existing issues**

4. **Create support request with**:
   - Version number
   - OS and distribution
   - Full error messages
   - Steps to reproduce
   - Logs (redacted)

## Prevention

**Regular maintenance**:
- Monitor logs daily
- Check disk space
- Review error rates
- Update regularly
- Test backups

**Monitoring**:
```bash
# Check service is running
sudo systemctl is-active {{BINARY_NAME}}

# Check disk space
df -h /var/lib/{{BINARY_NAME}}

# Check database size
sudo -u postgres psql -c "SELECT pg_size_pretty(pg_database_size('ops_system'));"

# Check log size
du -sh /var/log/{{BINARY_NAME}}
```
