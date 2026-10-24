# Backend CI/CD with Docker Hub

These files were imported from `portfolio-cicd-dockerhub.zip` and adapted to this repository. The current deployment keeps both the API and PostgreSQL in Docker Compose. CI/CD updates only the existing `api` service. The database service, Compose project, persistent volume, private environment, and Nginx configuration are preserved.

## Pipeline

The Rust package is `ad-portfolio-api`. Its executable remains `portfolio-api`, matching the Dockerfile entrypoint and the workflow's migration command. The Docker Hub repository is `adkhank999/ad-portfolio-api`; Cargo package names do not determine Docker Compose container or volume names.

Pull requests to `main` run shell syntax checks, Rust formatting, Clippy, migrations, and all Rust tests. GitHub Actions starts a disposable PostgreSQL 17 service with CI-only credentials and runs the normally ignored SQLx integration tests using `--include-ignored`. Production database credentials are never used in CI.

After tests pass on `main`, Actions builds the existing Dockerfile and publishes:

```text
docker.io/adkhank999/ad-portfolio-api:production
docker.io/adkhank999/ad-portfolio-api:sha-COMMIT_SHA
```

Deployment uses the commit-specific tag. It runs only when `BACKEND_DEPLOY_ENABLED=true`. Images are published on `main` even while deployment is disabled, so Docker Hub settings must be configured before that first publishing run.

## GitHub settings

In `Adnank999/ad-portfolio-backend`, open **Settings > Secrets and variables > Actions**.

| Repository secret | Value |
| --- | --- |
| `DOCKERHUB_TOKEN` | Docker Hub access token with permission to push the selected repository |
| `VPS_HOST` | VPS SSH hostname or IP |
| `VPS_USER` | Existing SSH account, for example `root` |
| `VPS_SSH_KEY` | Dedicated deployment private key in OpenSSH format |
| `VPS_KNOWN_HOSTS` | Independently verified SSH host-key entry |

| Repository variable | Required value or default |
| --- | --- |
| `DOCKERHUB_USERNAME` | `adkhank999` |
| `DOCKERHUB_IMAGE` | `adkhank999/ad-portfolio-api` |
| `BACKEND_DEPLOY_ENABLED` | Set `false` initially; enable after the first image is verified |
| `VPS_SSH_PORT` | `22` |
| `BACKEND_APP_DIR` | `/apps/ad-portfolio-backend` |
| `BACKEND_COMPOSE_PROJECT` | `ad-portfolio-backend`; must match the current deployment |
| `BACKEND_HEALTH_URL` | `http://127.0.0.1:8080/health/ready` |
| `DOCKER_PLATFORM` | `linux/amd64`; confirm the VPS architecture before publishing |

Create the Docker Hub repository before publishing. Use an access token rather than an account password. For a private image, sign in on the VPS with a separate read-only token using `docker login --password-stdin` through your trusted server session. The workflow does not transmit the push token to the VPS. SSH host keys must be verified separately; custom SSH ports require `[hostname]:port` entries.

The current runner and default image target are x86-64. Building for a different architecture requires a matching runner or Buildx emulation configuration before changing `DOCKER_PLATFORM`.

## Confirm the existing deployment

Run these on the VPS, replacing container names if they differ:

```bash
docker inspect ad-portfolio-backend-api-1 --format '{{ index .Config.Labels "com.docker.compose.project" }}'
docker inspect ad-portfolio-backend-api-1 --format '{{ index .Config.Labels "com.docker.compose.project.working_dir" }}'
docker inspect ad-portfolio-backend-db-1 --format '{{range .Mounts}}{{println .Type .Name .Destination}}{{end}}'
docker inspect ad-portfolio-backend-db-1 --format '{{.State.Health.Status}}'
command -v curl flock
docker compose version
```

Keep the inspected project name and database mount unchanged. The deployment account needs access to that existing Docker daemon, the app directory, and `.env.portfolio`. The database must already be running and healthy; the script refuses to start a new database automatically. PostgreSQL must have the health check from the existing Compose file.

Before enabling automatic deployment, take a fresh logical database dump with `pg_dump`, verify that it is readable with `pg_restore --list`, and keep a protected off-server copy. Also back up the current Compose configuration, private environment, and Nginx configuration. Keep all credentials and backups outside Git.

## Registry override and deployment

The base `docker-compose.yml` is deliberately unchanged. `deploy/docker-compose.registry.yml` overrides only the API image. The workflow uploads that small override and executes the update script over SSH; it does not sync or replace the server's base Compose file or `.env.portfolio`.

The script:

1. Locks deployment and validates the existing Compose configuration and database health.
2. Preserves the current API image under a local `rollback-local` tag.
3. Pulls the new API image and runs its embedded SQLx migrations using `run --no-deps`.
4. Recreates only `api` using `up --no-deps --no-build --pull never`.
5. Checks readiness and atomically records the successful image repository/tag in `.env.deploy`.
6. Restores the previous API image if activation or readiness fails after recreation begins.

If pulling or migrating fails, the script stops before recreating the running API. Failed deployments remain failed even if application-image rollback succeeds. Database schema migrations are not automatically reversed; migrations must remain compatible with the previous API during the rollback window. This process can briefly interrupt API traffic during recreation.

The script is streamed to Bash over SSH. The migration container explicitly disables interactive stdin so it cannot consume the remaining deployment commands; `-T` alone only disables the pseudo-terminal. The workflow also requires the final `API deployed successfully: sha-...` message before reporting success. Run `bash deploy/test-update-api.sh` to exercise streamed activation and migration failure with mocked Docker commands, without a database or Docker daemon.

The override retains the base API build configuration, but deployment pulls first and never requests a build. The `db` service and volume settings are not overridden. Deployment never runs `down`, volume deletion, or `--remove-orphans`.

## Manual operation and rollback

After the first successful deployment, the private `.env.deploy` contains the registry repository and selected tag. To inspect status on the VPS:

```bash
cd /apps/ad-portfolio-backend
docker compose --project-name ad-portfolio-backend --env-file .env.portfolio --env-file .env.deploy -f docker-compose.yml -f deploy/docker-compose.registry.yml ps
```

To deploy a known retained commit manually, run the reviewed update script from a checkout containing it:

```bash
bash deploy/update-api.sh sha-COMMIT_SHA docker.io/adkhank999/ad-portfolio-api /apps/ad-portfolio-backend ad-portfolio-backend http://127.0.0.1:8080/health/ready
```

Replace `COMMIT_SHA` with the full 40-character commit SHA. The registry override must already exist in the server app directory; the workflow uploads it before activation. Deploying an older image also runs its migration command: older binaries may reject a database containing newer migrations. For an application-only rollback, select a compatible retained image using the override and recreate only `api`; do not reset SQLx history or restore an old database over new writes.

## First activation

1. Confirm Docker Hub identity, VPS architecture, actual app directory, Compose project, and database volume.
2. Configure Docker Hub variables/token and the SSH secrets. Keep `BACKEND_DEPLOY_ENABLED=false`.
3. Push the reviewed files to `main`; verify tests and the published SHA image.
4. Configure VPS pull authentication if needed and take the backups described above.
5. Set `BACKEND_DEPLOY_ENABLED=true` and run the workflow on `main`.
6. Verify readiness, existing users/content, and the unchanged database container/volume.

No registry login, image publication, database migration, or VPS deployment is performed by installing these files.

References: [Docker Hub publishing with GitHub Actions](https://docs.docker.com/guides/gha/), [Docker Hub access tokens](https://docs.docker.com/security/access-tokens/), and [Docker Compose up](https://docs.docker.com/reference/cli/docker/compose/up/).
