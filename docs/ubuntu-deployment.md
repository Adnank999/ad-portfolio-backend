# Ubuntu deployment: native PostgreSQL, Docker API

The deployment topology is:

```text
Browser -> HTTPS reverse proxy on Ubuntu -> API container (127.0.0.1:8080)
                                            |
                                            v
                              PostgreSQL on Ubuntu (127.0.0.1:5432)
```

`docker-compose.yml` runs only `api`. Linux `network_mode: host` lets that container reach Ubuntu's loopback address, so PostgreSQL needs no public port, Docker bridge address, or container volume. The API also binds to loopback; run the reverse proxy on the host and forward to `http://127.0.0.1:8080`. Host networking shares the host network namespace. This Compose deployment targets Docker Engine on Ubuntu; Docker Desktop requires its optional host-networking feature.

The existing database container was verified as PostgreSQL 17.11, database `portfolio`, user `portfolio`, published locally on port `55432`. Install PostgreSQL **17** on Ubuntu to keep the same major version. Use a logical `pg_dump`/`pg_restore` migration; do not copy Docker's raw data directory between operating systems or PostgreSQL installations.

Commands below use PowerShell on the current source computer and Bash on Ubuntu. Docker Engine with the Compose plugin and SSH access must already be available on Ubuntu. Replace `deploy@YOUR_SERVER` with your SSH account and server address.

## 1. Stop writes and export the current database

Put the site into maintenance mode and stop all API processes, workers, and scheduled tasks that write to the source database. Keep them stopped until cutover is complete. A dump is a consistent snapshot, but writes made after that snapshot would not be migrated. Keep the database container running for the export.

On the source computer, find the PostgreSQL container:

```powershell
docker ps --format 'table {{.Names}}\t{{.Image}}\t{{.Ports}}'
$dbContainer = 'ad-portfolio-api-db-1'
docker exec $dbContainer psql -X -U portfolio -d portfolio -c 'SHOW server_version;'
```

Adjust `$dbContainer` if the name differs. Run commands separately and stop if any fails. Export **inside the container**, then copy the binary archive out; this avoids binary corruption from older PowerShell output redirection:

```powershell
$backupDirectory = Join-Path $env:USERPROFILE 'portfolio-migration'
New-Item -ItemType Directory -Force -Path $backupDirectory | Out-Null
docker exec $dbContainer pg_dump -U portfolio -d portfolio --format=custom --file=/tmp/portfolio.dump
if ($LASTEXITCODE -ne 0) { throw 'Database export failed' }
docker exec $dbContainer pg_restore --list /tmp/portfolio.dump
if ($LASTEXITCODE -ne 0) { throw 'Database archive is unreadable' }
docker cp "${dbContainer}:/tmp/portfolio.dump" (Join-Path $backupDirectory 'portfolio.dump')
if ($LASTEXITCODE -ne 0) { throw 'Database archive copy failed' }
Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $backupDirectory 'portfolio.dump')
```

Save the SHA-256 hash and record these source counts for comparison after restoring:

```powershell
docker exec $dbContainer psql -X -v ON_ERROR_STOP=1 -U portfolio -d portfolio -c "SELECT 'users' AS table_name, count(*) FROM users UNION ALL SELECT 'roles', count(*) FROM roles UNION ALL SELECT 'user_roles', count(*) FROM user_roles UNION ALL SELECT 'content_items', count(*) FROM content_items UNION ALL SELECT 'content_slugs', count(*) FROM content_slugs UNION ALL SELECT '_sqlx_migrations', count(*) FROM _sqlx_migrations ORDER BY table_name;"
docker exec $dbContainer psql -X -v ON_ERROR_STOP=1 -U portfolio -d portfolio -c "SELECT kind, status, count(*) FROM content_items GROUP BY kind, status ORDER BY kind, status;"
```

Transfer the archive over SSH:

```powershell
ssh deploy@YOUR_SERVER 'mkdir -p ~/portfolio-migration && chmod 700 ~/portfolio-migration'
scp (Join-Path $backupDirectory 'portfolio.dump') deploy@YOUR_SERVER:~/portfolio-migration/portfolio.dump
```

On Ubuntu:

```bash
chmod 600 "$HOME/portfolio-migration/portfolio.dump"
sha256sum "$HOME/portfolio-migration/portfolio.dump"
```

Confirm that the hash matches the Windows result before continuing. The archive contains account password hashes, session records, and private content; keep it private and outside the repository. Keep a separate protected backup as well.

If the source Docker database is on Linux instead, use the same `docker exec ... pg_dump` and `docker cp` steps in Bash with your container name and a private backup directory. Avoid allocating a TTY for binary exports. If source and destination are the same server, skip `scp` and place the archive in the destination directory directly.

## 2. Install PostgreSQL 17 directly on Ubuntu

Use PostgreSQL's official Ubuntu APT repository to select version 17 explicitly:

```bash
sudo apt-get update
sudo apt-get install -y postgresql-common ca-certificates curl
sudo /usr/share/postgresql-common/pgdg/apt.postgresql.org.sh
sudo apt-get update
sudo apt-get install -y postgresql-17 postgresql-client-17
sudo systemctl enable --now postgresql
pg_lsclusters
```

Confirm the `17 main` cluster is online on **5432**. If an existing cluster already occupies that port, identify which databases it serves before changing anything. Use the actual PostgreSQL 17 port consistently in the remaining commands and `DATABASE_URL`; do not remove an existing cluster. The old Docker source normally uses host port 55432, so it can coexist during migration.

For a newly provisioned dedicated cluster, edit `/etc/postgresql/17/main/postgresql.conf` using `sudoedit` and set:

```conf
listen_addresses = '127.0.0.1'
port = 5432
password_encryption = 'scram-sha-256'
```

In `/etc/postgresql/17/main/pg_hba.conf`, ensure this rule appears before any broader matching host rules. Preserve the local `postgres` peer-authentication rule:

```conf
host    portfolio    portfolio    127.0.0.1/32    scram-sha-256
```

Restart the cluster and confirm its settings:

```bash
sudo systemctl restart postgresql@17-main
sudo -u postgres psql -X -p 5432 -c 'SHOW server_version; SHOW listen_addresses; SHOW port;'
```

No inbound firewall opening for PostgreSQL is needed; the app uses the same host's loopback address.

## 3. Create a native database and restore the archive

Create a dedicated login and an **empty** database. The commands prompt for the new database password and do not grant superuser, role-creation, or database-creation privileges:

```bash
sudo -u postgres createuser -p 5432 --pwprompt portfolio
sudo -u postgres createdb -p 5432 --owner=portfolio --template=template0 --encoding=UTF8 portfolio
```

Choose a new strong password. A password generated with `openssl rand -hex 32` is also safe to place in a database URL without additional encoding. If the role or database already exists, inspect it first and use a separate empty destination database rather than overwriting existing data.

Restore the complete archive **before** running the application's migration command:

```bash
pg_restore \
  --host=127.0.0.1 --port=5432 --username=portfolio --password \
  --dbname=portfolio --no-owner --no-privileges \
  --exit-on-error --single-transaction \
  "$HOME/portfolio-migration/portfolio.dump"
```

Enter the new password when prompted. Check that `pg_restore` exits successfully. `--no-owner --no-privileges` makes the new `portfolio` login own restored objects without importing the old Docker role's privileges. The single-transaction restore rolls back if any SQL command fails. The new database login is distinct from the application's users, whose records and password hashes are restored unchanged.

The dump includes schema, content, users, role assignments, sessions, indexes, sequences, and `_sqlx_migrations`. Do not pre-create the application tables or rerun imports/bootstrap commands as a substitute for restoring. Images/videos referenced through R2 remain in R2; keep the corresponding frontend upload configuration. Any separately stored local files need their own transfer.

Connect to the restored database:

```bash
psql -X --host=127.0.0.1 --port=5432 --username=portfolio --password --dbname=portfolio
```

Run these queries and compare the results with the source **before** applying pending application migrations:

```sql
SELECT 'users' AS table_name, count(*) FROM users
UNION ALL SELECT 'roles', count(*) FROM roles
UNION ALL SELECT 'user_roles', count(*) FROM user_roles
UNION ALL SELECT 'content_items', count(*) FROM content_items
UNION ALL SELECT 'content_slugs', count(*) FROM content_slugs
UNION ALL SELECT '_sqlx_migrations', count(*) FROM _sqlx_migrations
ORDER BY table_name;

SELECT kind, status, count(*)
FROM content_items
GROUP BY kind, status
ORDER BY kind, status;

SELECT version, description, success
FROM _sqlx_migrations
ORDER BY version;

ANALYZE;
```

Exit `psql` with `\q`. Continue only if the counts match, migration entries are successful, and no restore errors occurred.

## 4. Configure and start only the app in Docker

Copy the current backend source to Ubuntu. From its `Backend` directory, create `.env.portfolio` only if it does not already exist:

```bash
cp -n .env.portfolio.example .env.portfolio
chmod 600 .env.portfolio
nano .env.portfolio
```

Set these values for the destination:

```dotenv
DATABASE_URL=postgresql://portfolio:YOUR_NEW_URL_ENCODED_PASSWORD@127.0.0.1:5432/portfolio
SERVICE_PORT=8080
FRONTEND_URL=https://your-portfolio.example
API_PUBLIC_URL=https://api.your-portfolio.example
CORS_ORIGINS=https://your-portfolio.example
```

Percent-encode reserved characters in the password portion of `DATABASE_URL`. For environment values containing a literal `$`, use single quotes around the entire value so Compose does not interpolate it. Carry over your intended SMTP/AI settings privately. The native database does not use `POSTGRES_PASSWORD`; its password was set by `createuser`.

Compose fixes `SERVICE_HOST=127.0.0.1` for the Ubuntu reverse proxy. No `ports:` mapping is needed with host networking. Use the explicit `--env-file` option so Compose does not read the upstream demo's unrelated `.env` file for interpolation.

```bash
docker compose --env-file .env.portfolio config --quiet
docker compose --env-file .env.portfolio build api
docker compose --env-file .env.portfolio run --rm api migrate
docker compose --env-file .env.portfolio up -d api
docker compose --env-file .env.portfolio ps
docker compose --env-file .env.portfolio logs --tail=100 api
curl --fail http://127.0.0.1:8080/health/live
curl --fail http://127.0.0.1:8080/health/ready
```

Run these commands separately and stop if a command fails. `migrate` uses the restored SQLx history to apply only pending migrations. If it reports checksum mismatches, stop and recover the matching migration files instead of editing migration history. On a fresh installation with no source database, skip the export/restore sections and run `migrate` against the new empty database, then follow the owner bootstrap instructions in `PORTFOLIO.md`.

This setup uses a non-superuser database owner for both migrations and the API. If you split migration and runtime logins later, apply schema changes with the owner and grant the runtime login only the required table/sequence privileges.

## 5. Verify cutover and keep a rollback path

Keep maintenance mode enabled until the native database checks, readiness endpoint, reverse proxy, existing owner login, public content, admin editing, and media URLs work. Make any verification edits only in the destination and record them if rollback might be needed. Update the frontend server's `PORTFOLIO_API_URL` to the reachable API address. A frontend process on Ubuntu can use `http://127.0.0.1:8080`; a separate bridge-networked container must reach the host through its own configured networking/proxy, since its loopback belongs to itself.

Once verified, switch traffic to the new deployment and end maintenance mode. Keep the old PostgreSQL container and its volume intact during the rollback window; it may be stopped after verification. Do not run `docker compose down -v`, `docker volume prune`, or `--remove-orphans` against the source during migration. The old `db` container can appear as an orphan because the updated Compose file now contains only `api`.

Before reopening writes, rollback means stopping the new API and restarting the original deployment with its original source database URL. After the new database has accepted writes, the databases have diverged: stop writes and transfer/reconcile that new data before reverting. Simply pointing the API back would discard those new changes.

Set up regular native PostgreSQL backups with protected off-server copies and verify restoration periodically. Monitor disk space and the PostgreSQL service, and keep version 17 packages patched. Backups made during this transfer do not replace an ongoing backup schedule.

## References

- [PostgreSQL packages for Ubuntu](https://www.postgresql.org/download/linux/ubuntu/)
- [PostgreSQL 17 pg_dump](https://www.postgresql.org/docs/17/app-pgdump.html)
- [PostgreSQL 17 pg_restore](https://www.postgresql.org/docs/17/app-pgrestore.html)
- [PostgreSQL 17 pg_hba.conf](https://www.postgresql.org/docs/17/auth-pg-hba-conf.html)
- [Docker host networking](https://docs.docker.com/engine/network/drivers/host/)
