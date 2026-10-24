#!/usr/bin/env bash
set -Eeuo pipefail
umask 077

NEW_TAG="${1:?Missing immutable image tag}"
IMAGE_REPO="${2:?Missing image repository}"
APP_DIR="${3:-/root/apps/ad-portfolio-backend}"
PROJECT="${4:-ad-portfolio-backend}"
HEALTH_URL="${5:-http://127.0.0.1:8080/health/ready}"

[[ "$NEW_TAG" =~ ^sha-[0-9a-f]{40}$ ]] || { echo 'Invalid image tag' >&2; exit 2; }
[[ "$IMAGE_REPO" =~ ^docker\.io/[a-z0-9][a-z0-9._-]*/[a-z0-9][a-z0-9._-]*$ ]] || { echo 'Invalid Docker Hub repository' >&2; exit 2; }
[[ "$PROJECT" =~ ^[a-z0-9][a-z0-9_-]*$ ]] || { echo 'Invalid Compose project' >&2; exit 2; }
[[ "$HEALTH_URL" =~ ^http://127\.0\.0\.1:[0-9]+/ ]] || { echo 'Health URL must use host loopback' >&2; exit 2; }
APP_DIR="$(realpath -e -- "$APP_DIR")"
[[ "$APP_DIR" != / ]] || exit 2
cd "$APP_DIR"

[[ -f .env.portfolio ]] || { echo '.env.portfolio is missing' >&2; exit 1; }
[[ -f docker-compose.yml ]] || { echo 'docker-compose.yml is missing' >&2; exit 1; }
[[ -f deploy/docker-compose.registry.yml ]] || { echo 'Registry Compose override is missing' >&2; exit 1; }

exec 9>".deploy.lock"
flock -n 9 || { echo 'Another API deployment is running' >&2; exit 1; }

if [[ ! -f .env.deploy ]]; then
  : > .env.deploy
fi
chmod 600 .env.deploy

DEPLOY_TAG="$NEW_TAG"
dc() {
  IMAGE_REPOSITORY="$IMAGE_REPO" IMAGE_TAG="$DEPLOY_TAG" docker compose \
    --project-name "$PROJECT" \
    --env-file .env.portfolio --env-file .env.deploy \
    -f docker-compose.yml -f deploy/docker-compose.registry.yml "$@"
}

verify_private_api_port() {
  command -v python3 > /dev/null || { echo 'DEPLOY BLOCKED: python3 is required to inspect API ports' >&2; exit 1; }
  dc config --format json | python3 -c '
import json
import sys

try:
    api = json.load(sys.stdin)["services"]["api"]
    ports = api.get("ports", [])
    port = ports[0] if isinstance(ports, list) and len(ports) == 1 else None
except (KeyError, ValueError, TypeError, AttributeError):
    print("DEPLOY BLOCKED: Cannot inspect effective API port configuration", file=sys.stderr)
    sys.exit(1)

if (
    api.get("network_mode") == "host"
    or not isinstance(port, dict)
    or port.get("host_ip") != "127.0.0.1"
    or port.get("protocol", "tcp") != "tcp"
    or not str(port.get("published", "")).isdigit()
    or not 1 <= int(port["published"]) <= 65535
):
    print(
        "DEPLOY BLOCKED: API must publish exactly one TCP port bound to 127.0.0.1 without host networking",
        file=sys.stderr,
    )
    sys.exit(1)

print("Private API port verified: 127.0.0.1:" + str(port["published"]))
'
}

save_state() {
  local state_file
  state_file="$(mktemp "$APP_DIR/.env.deploy.XXXXXX")"
  printf 'IMAGE_REPOSITORY=%s\nIMAGE_TAG=%s\n' "$IMAGE_REPO" "$DEPLOY_TAG" > "$state_file"
  mv -f -- "$state_file" .env.deploy
}

healthy() {
  local attempt
  for ((attempt = 0; attempt < 20; attempt += 1)); do
    if [[ -n "$(dc ps -q api)" ]] &&
      curl -fsS --connect-timeout 2 --max-time 8 -o /dev/null "$HEALTH_URL"; then
      return 0
    fi
    sleep 2
  done
  return 1
}

dc config --quiet
verify_private_api_port
DATABASE_CONTAINER="$(dc ps -q db)"
[[ -n "$DATABASE_CONTAINER" ]] || { echo 'Existing database container is not running in this Compose project' >&2; exit 1; }
[[ "$(docker inspect "$DATABASE_CONTAINER" --format '{{.State.Health.Status}}')" == healthy ]] ||
  { echo 'Existing PostgreSQL container is not healthy' >&2; exit 1; }

PREVIOUS_ID=''
API_CONTAINER="$(dc ps -a -q api)"
if [[ -n "$API_CONTAINER" ]]; then
  PREVIOUS_ID="$(docker inspect "$API_CONTAINER" --format '{{.Image}}')"
  docker tag "$PREVIOUS_ID" "$IMAGE_REPO:rollback-local"
fi

dc pull api
dc run --rm --no-deps --pull never -T --interactive=false api migrate

rollback() {
  local failure="$?"
  trap - ERR
  set +e
  echo 'API activation failed; attempting application-image rollback' >&2
  if [[ -n "$PREVIOUS_ID" ]]; then
    DEPLOY_TAG='rollback-local'
    if dc up -d --no-deps --no-build --pull never api && healthy; then
      save_state
      echo 'Previous API image is healthy; database migrations were not reversed' >&2
    else
      echo 'API rollback needs manual recovery; database was left intact' >&2
    fi
  else
    echo 'No previous API image is available; manual recovery required' >&2
  fi
  exit "$failure"
}
trap rollback ERR

dc up -d --no-deps --no-build --pull never api
healthy
save_state

trap - ERR
echo "API deployed successfully: $NEW_TAG"
