# Upgrade Guide for {{BINARY_NAME}}

## Table of Contents
- [Pre-Upgrade Checklist](#pre-upgrade-checklist)
- [Backup Procedures](#backup-procedures)
- [Upgrade Methods](#upgrade-methods)
- [Post-Upgrade Steps](#post-upgrade-steps)
- [Rollback Procedures](#rollback-procedures)
- [Version-Specific Notes](#version-specific-notes)

## Pre-Upgrade Checklist

Before upgrading, ensure you have:

- [ ] Read the release notes for the new version
- [ ] Completed a full backup of the database
- [ ] Backed up configuration files
- [ ] Noted your current version
- [ ] Scheduled maintenance window (if needed)
- [ ] Tested the upgrade in a staging environment
- [ ] Notified users of potential downtime

## Backup Procedures

### Automated Backup

Use the included backup script:

```bash
cd linux-{{PLATFORM}}/
sudo ./scripts/backup.sh
```

This will create:
- Configuration backup
- Data directory backup
- Database dump (if PostgreSQL is accessible)

Backups are stored in `/var/backups/{{BINARY_NAME}}/`.

### Manual Backup

```bash
# 1. Backup configuration
sudo cp -r /etc/{{BINARY_NAME}} /tmp/{{BINARY_NAME}}-config-backup

# 2. Backup data directory
sudo cp -r /var/lib/{{BINARY_NAME}} /tmp/{{BINARY_NAME}}-data-backup

# 3. Backup database
sudo -u postgres pg_dump ops_system > /tmp/{{BINARY_NAME}}-db-backup.sql

# 4. Note current version
cat /var/lib/{{BINARY_NAME}}/VERSION
```

### Verify Backup

```bash
# Check backup files exist
ls -lh /tmp/{{BINARY_NAME}}-*-backup*

# Verify database backup
head -20 /tmp/{{BINARY_NAME}}-db-backup.sql
```

## Upgrade Methods

### Method 1: Automated Upgrade (Recommended)

The update script handles:
- Stopping the service
- Backing up the current binary
- Installing the new version
- Restarting the service
- Cleanup of old backups

```bash
# 1. Extract the new version
tar -xzf {{BINARY_NAME}}-{{VERSION}}-linux-{{PLATFORM}}.tar.gz
cd linux-{{PLATFORM}}

# 2. Run the update script
sudo ./scripts/update.sh
```

### Method 2: Manual Upgrade

```bash
# 1. Stop the service
sudo systemctl stop {{BINARY_NAME}}

# 2. Backup current binary
sudo cp /usr/local/bin/{{BINARY_NAME}} /usr/local/bin/{{BINARY_NAME}}.old

# 3. Install new binary
sudo cp bin/{{BINARY_NAME}} /usr/local/bin/
sudo chmod +x /usr/local/bin/{{BINARY_NAME}}

# 4. Update migrations (if needed)
sudo cp migrations/* /var/lib/{{BINARY_NAME}}/migrations/

# 5. Run database migrations
export OPS_DATABASE_URL="..."  # Your database URL
sqlx migrate run --database-url $OPS_DATABASE_URL

# 6. Start the service
sudo systemctl start {{BINARY_NAME}}

# 7. Verify
sudo systemctl status {{BINARY_NAME}}
```

### Method 3: Docker Upgrade

```bash
# 1. Stop current container
docker compose -f docker/docker-compose.yml down

# 2. Pull or build new image
cd docker/
docker build -t {{BINARY_NAME}}:{{VERSION}} .

# 3. Update docker-compose.yml if needed
# 4. Start new version
docker compose up -d

# 5. Verify
docker compose logs -f
```

## Post-Upgrade Steps

### 1. Verify Service Status

```bash
sudo systemctl status {{BINARY_NAME}}
```

The service should show as "active (running)".

### 2. Check Logs

```bash
sudo journalctl -u {{BINARY_NAME}} -n 100
```

Look for:
- Startup errors
- Database migration issues
- Configuration warnings

### 3. Run Health Checks

```bash
# Health endpoint
curl http://localhost:3000/health

# Ready endpoint
curl http://localhost:3000/ready
```

### 4. Verify Database Migrations

```bash
# Check migration version
psql "postgresql://user:pass@localhost:5432/dbname" -c "SELECT * FROM _sqlx_migrations;"
```

### 5. Test Functionality

- Test API endpoints
- Verify user authentication works
- Check audit logs are being created
- Verify database queries work

### 6. Monitor Resources

```bash
# Check memory usage
systemctl show {{BINARY_NAME}} -p MemoryCurrent

# Check process status
ps aux | grep {{BINARY_NAME}}
```

### 7. Clean Up Old Backups

```bash
# Remove backups older than 30 days
find /var/backups/{{BINARY_NAME}} -mtime +30 -delete

# Remove old binary (if everything works)
sudo rm /usr/local/bin/{{BINARY_NAME}}.old
```

## Rollback Procedures

### If the Automated Update Fails

The update script automatically backs up the old binary. To rollback:

```bash
# 1. Stop the service
sudo systemctl stop {{BINARY_NAME}}

# 2. Restore old binary
sudo cp /usr/local/bin/{{BINARY_NAME}}.old /usr/local/bin/{{BINARY_NAME}}

# 3. Restart service
sudo systemctl start {{BINARY_NAME}}

# 4. Verify
sudo systemctl status {{BINARY_NAME}}
```

### Manual Rollback

```bash
# 1. Stop the service
sudo systemctl stop {{BINARY_NAME}}

# 2. Restore binary from backup
sudo cp /path/to/backup/{{BINARY_NAME}} /usr/local/bin/{{BINARY_NAME}}
sudo chmod +x /usr/local/bin/{{BINARY_NAME}}

# 3. Restore configuration (if needed)
sudo cp -r /tmp/{{BINARY_NAME}}-config-backup/* /etc/{{BINARY_NAME}}/

# 4. Restore database (if needed)
sudo -u postgres psql ops_system < /tmp/{{BINARY_NAME}}-db-backup.sql

# 5. Restart service
sudo systemctl start {{BINARY_NAME}}
```

### Database Rollback

If database migrations need to be rolled back:

```bash
# Warning: This can cause data loss!
# Only rollback migrations if you're certain

# Check current migration version
psql "postgresql://user:pass@localhost/dbname" -c "SELECT * FROM _sqlx_migrations;"

# Rollback to specific version (manual SQL required)
# You'll need to write and execute the reverse SQL manually
```

## Version-Specific Notes

### Upgrading to {{VERSION}}

Check for any specific instructions in the release notes.

### Common Upgrade Scenarios

#### Configuration Changes

If the new version requires configuration changes:

1. Compare old and new `.env.example`:
   ```bash
   diff /etc/{{BINARY_NAME}}/env config/.env.example
   ```

2. Add new required variables to your configuration

3. Restart the service

#### Database Schema Changes

For major version upgrades with schema changes:

1. **Always backup first**
2. Review migration files in the `migrations/` directory
3. Test migrations in staging first
4. Plan for potential downtime
5. Have rollback plan ready

#### Breaking Changes

If the new version has breaking changes:

1. Review the API changelog
2. Update any client applications
3. Update integrations
4. Test thoroughly before upgrading

## Zero-Downtime Upgrade (Advanced)

For high-availability setups:

### Using Multiple Instances

```bash
# 1. Start new version on different port
export OPS_SERVER_ADDR=0.0.0.0:3001
/usr/local/bin/{{BINARY_NAME}}-new &

# 2. Health check new instance
curl http://localhost:3001/health

# 3. Switch load balancer to new instance

# 4. Stop old instance
sudo systemctl stop {{BINARY_NAME}}
```

### Using Blue-Green Deployment

```bash
# 1. Deploy "green" instance
# 2. Test green instance thoroughly
# 3. Switch traffic from blue to green
# 4. Monitor green instance
# 5. Decommission blue instance
```

## Getting Help

If you encounter issues during upgrade:

1. Check logs: `sudo journalctl -u {{BINARY_NAME}} -n 200`
2. Review [TROUBLESHOOTING.md](TROUBLESHOOTING.md)
3. Search existing issues
4. Contact support with:
   - Current version
   - Target version
   - Error messages
   - Logs (redacted)
   - Steps taken

## Best Practices

1. **Always backup before upgrading**
2. **Test in staging first**
3. **Schedule maintenance windows for major upgrades**
4. **Keep a rollback plan ready**
5. **Monitor the service after upgrade**
6. **Document any custom configurations**
7. **Subscribe to release announcements**
8. **Review changelogs before upgrading**
