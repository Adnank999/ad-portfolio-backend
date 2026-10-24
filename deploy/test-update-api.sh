#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_PATH="$(realpath -- "${1:-$(dirname -- "${BASH_SOURCE[0]}")/update-api.sh}")"
TEMP_ROOT="$(realpath -- "${TMPDIR:-/tmp}")"
TEST_DIR="$(mktemp -d "$TEMP_ROOT/portfolio-deploy-test.XXXXXX")"
[[ "$TEST_DIR" == "$TEMP_ROOT"/portfolio-deploy-test.* && "$TEST_DIR" != "$TEMP_ROOT" ]] || exit 1
trap 'rm -rf -- "$TEST_DIR"' EXIT

docker() {
  local command=''
  case "$1" in
    inspect)
      case "$2" in
        database-id) printf '%s\n' healthy ;;
        api-id) printf '%s\n' sha256:previous-image ;;
        *) return 1 ;;
      esac
      return 0
      ;;
    tag) return 0 ;;
    compose) shift ;;
    *) return 1 ;;
  esac

  while (($#)); do
    case "$1" in
      config|ps|pull|run|up)
        command="$1"
        shift
        break
        ;;
    esac
    shift
  done

  case "$command" in
    config|pull) return 0 ;;
    ps)
      if [[ "${*: -1}" == db ]]; then
        printf '%s\n' database-id
      else
        printf '%s\n' api-id
      fi
      ;;
    run)
      printf '%s\n' migrate >> "$MOCK_EVENTS"
      if [[ " $* " != *' --interactive=false '* ]]; then
        cat > /dev/null
      fi
      if [[ "$MOCK_SCENARIO" == migration-failure ]]; then
        return 42
      fi
      ;;
    up) printf '%s\n' activate >> "$MOCK_EVENTS" ;;
    *) return 1 ;;
  esac
}

flock() { return 0; }
curl() { return 0; }
export -f docker flock curl

IMAGE_TAG="sha-$(printf 'a%.0s' {1..40})"
IMAGE_REPOSITORY='docker.io/adkhank999/ad-portfolio-api'

for MOCK_SCENARIO in success migration-failure; do
  APP_DIR="$TEST_DIR/$MOCK_SCENARIO"
  mkdir -p "$APP_DIR/deploy"
  touch "$APP_DIR/.env.portfolio" "$APP_DIR/docker-compose.yml" "$APP_DIR/deploy/docker-compose.registry.yml"
  MOCK_EVENTS="$APP_DIR/events"
  export MOCK_EVENTS MOCK_SCENARIO

  STATUS=0
  bash -s -- "$IMAGE_TAG" "$IMAGE_REPOSITORY" "$APP_DIR" ad-portfolio-backend \
    < "$SCRIPT_PATH" > "$APP_DIR/output" 2>&1 || STATUS="$?"

  grep -Fxq migrate "$MOCK_EVENTS"
  if [[ "$MOCK_SCENARIO" == success ]]; then
    [[ "$STATUS" == 0 ]]
    grep -Fxq activate "$MOCK_EVENTS"
    grep -Fxq "IMAGE_TAG=$IMAGE_TAG" "$APP_DIR/.env.deploy"
    grep -Fxq "IMAGE_REPOSITORY=$IMAGE_REPOSITORY" "$APP_DIR/.env.deploy"
    grep -Fxq "API deployed successfully: $IMAGE_TAG" "$APP_DIR/output"
  else
    [[ "$STATUS" == 42 ]]
    ! grep -Fxq activate "$MOCK_EVENTS"
    [[ ! -s "$APP_DIR/.env.deploy" ]]
    ! grep -Fq 'API deployed successfully:' "$APP_DIR/output"
  fi
  printf 'PASS: streamed deployment %s\n' "$MOCK_SCENARIO"
done
