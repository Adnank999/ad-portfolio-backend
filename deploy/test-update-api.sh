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
    config)
      if [[ " $* " == *' --format json '* ]]; then
        local host_ip='127.0.0.1'
        local protocol='tcp'
        case "$MOCK_SCENARIO" in
          public-binding) host_ip='0.0.0.0' ;;
          ipv6-binding) host_ip='::' ;;
          udp-binding) protocol='udp' ;;
          missing-host)
            printf '%s\n' '{"services":{"api":{"ports":[{"target":8080,"published":"8080"}]}}}'
            return 0
            ;;
          multiple-bindings)
            printf '%s\n' '{"services":{"api":{"ports":[{"host_ip":"127.0.0.1","published":"8080"},{"host_ip":"0.0.0.0","published":"8081"}]}}}'
            return 0
            ;;
          host-network)
            printf '%s\n' '{"services":{"api":{"network_mode":"host","ports":[{"host_ip":"127.0.0.1","published":"8080"}]}}}'
            return 0
            ;;
          malformed-json)
            printf '%s\n' 'invalid-json-with-rendered-secret'
            return 0
            ;;
          malformed-ports)
            printf '%s\n' '{"services":{"api":{"ports":["invalid-json-with-rendered-secret"]}}}'
            return 0
            ;;
        esac
        printf '{"services":{"api":{"ports":[{"host_ip":"%s","target":8080,"published":"8080","protocol":"%s"}]}}}\n' "$host_ip" "$protocol"
      fi
      ;;
    pull) printf '%s\n' pull >> "$MOCK_EVENTS" ;;
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

for MOCK_SCENARIO in success migration-failure public-binding ipv6-binding missing-host multiple-bindings udp-binding host-network malformed-json malformed-ports; do
  APP_DIR="$TEST_DIR/$MOCK_SCENARIO"
  mkdir -p "$APP_DIR/deploy"
  touch "$APP_DIR/.env.portfolio" "$APP_DIR/docker-compose.yml" "$APP_DIR/deploy/docker-compose.registry.yml"
  MOCK_EVENTS="$APP_DIR/events"
  export MOCK_EVENTS MOCK_SCENARIO

  STATUS=0
  bash -s -- "$IMAGE_TAG" "$IMAGE_REPOSITORY" "$APP_DIR" ad-portfolio-backend \
    < "$SCRIPT_PATH" > "$APP_DIR/output" 2>&1 || STATUS="$?"

  case "$MOCK_SCENARIO" in
    success)
      [[ "$STATUS" == 0 ]]
      grep -Fxq migrate "$MOCK_EVENTS"
      grep -Fxq activate "$MOCK_EVENTS"
      grep -Fxq "IMAGE_TAG=$IMAGE_TAG" "$APP_DIR/.env.deploy"
      grep -Fxq "IMAGE_REPOSITORY=$IMAGE_REPOSITORY" "$APP_DIR/.env.deploy"
      grep -Fxq 'Private API port verified: 127.0.0.1:8080' "$APP_DIR/output"
      grep -Fxq "API deployed successfully: $IMAGE_TAG" "$APP_DIR/output"
      ;;
    migration-failure)
      [[ "$STATUS" == 42 ]]
      grep -Fxq migrate "$MOCK_EVENTS"
      ! grep -Fxq activate "$MOCK_EVENTS"
      [[ ! -s "$APP_DIR/.env.deploy" ]]
      ! grep -Fq 'API deployed successfully:' "$APP_DIR/output"
      ;;
    *)
      [[ "$STATUS" != 0 ]]
      [[ ! -e "$MOCK_EVENTS" || ! -s "$MOCK_EVENTS" ]]
      [[ ! -s "$APP_DIR/.env.deploy" ]]
      grep -Fq 'DEPLOY BLOCKED' "$APP_DIR/output"
      ! grep -Fq 'invalid-json-with-rendered-secret' "$APP_DIR/output"
      ;;
  esac
  printf 'PASS: streamed deployment %s\n' "$MOCK_SCENARIO"
done
