#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
source "${SCRIPT_DIR}/lib.sh"

require_root

identity_env="${VAPOR_CONFIG_DIR}/identity.env"
registry_env="${VAPOR_CONFIG_DIR}/registry.env"
platform_env="${VAPOR_CONFIG_DIR}/platform.env"
artifact_env="${VAPOR_CONFIG_DIR}/artifact.env"
build_cache_env="${VAPOR_CONFIG_DIR}/build-cache.env"
identity_key="VAPOR_IDENTITY_AUTHZ_SERVICE_TOKEN"
registry_key="VAPOR_REGISTRY_IDENTITY_AUTHZ_TOKEN"
identity_platform_key="VAPOR_IDENTITY_PLATFORM_ADMIN_AUTHZ_TOKEN"
platform_token_key="VAPOR_PLATFORM_IDENTITY_AUTHZ_TOKEN"
identity_artifact_key="VAPOR_IDENTITY_ARTIFACT_ADMIN_AUTHZ_TOKEN"
artifact_token_key="VAPOR_ARTIFACT_IDENTITY_AUTHZ_TOKEN"
identity_build_cache_key="VAPOR_IDENTITY_BUILD_CACHE_ADMIN_AUTHZ_TOKEN"
build_cache_token_key="VAPOR_BUILD_CACHE_IDENTITY_AUTHZ_TOKEN"
build_cache_orchestrator_key="VAPOR_BUILD_CACHE_ORCHESTRATOR_TOKEN"
identity_request_key="VAPOR_IDENTITY_PLATFORM_REQUEST_AUTHZ_TOKEN"
platform_request_key="VAPOR_PLATFORM_IDENTITY_REQUEST_AUTHZ_TOKEN"

if [ ! -f "$identity_env" ] || [ ! -f "$registry_env" ] || [ ! -f "$platform_env" ] || [ ! -f "$artifact_env" ] || [ ! -f "$build_cache_env" ]; then
  echo "error: all service env files must exist before configuring authorization" >&2
  exit 1
fi

read_env_value() {
  local file="$1"
  local key="$2"
  awk -F= -v key="$key" '
    $1 == key {
      value = substr($0, index($0, "=") + 1)
      if (value ~ /^".*"$/) value = substr(value, 2, length(value) - 2)
      if (value ~ /^\047.*\047$/) value = substr(value, 2, length(value) - 2)
      print value
      exit
    }
  ' "$file"
}

valid_token() {
  [[ "$1" =~ ^[A-Fa-f0-9]{64}$ ]]
}

identity_token="$(read_env_value "$identity_env" "$identity_key")"
registry_token="$(read_env_value "$registry_env" "$registry_key")"
identity_platform_token="$(read_env_value "$identity_env" "$identity_platform_key")"
platform_token="$(read_env_value "$platform_env" "$platform_token_key")"
identity_artifact_token="$(read_env_value "$identity_env" "$identity_artifact_key")"
artifact_token="$(read_env_value "$artifact_env" "$artifact_token_key")"
identity_build_cache_token="$(read_env_value "$identity_env" "$identity_build_cache_key")"
build_cache_token="$(read_env_value "$build_cache_env" "$build_cache_token_key")"
build_cache_orchestrator_token="$(read_env_value "$build_cache_env" "$build_cache_orchestrator_key")"
identity_request_token="$(read_env_value "$identity_env" "$identity_request_key")"
platform_request_token="$(read_env_value "$platform_env" "$platform_request_key")"

if [ -n "$identity_token" ] && ! valid_token "$identity_token"; then
  echo "error: ${identity_key} must be a 64-character hex token" >&2
  exit 1
fi
if [ -n "$registry_token" ] && ! valid_token "$registry_token"; then
  echo "error: ${registry_key} must be a 64-character hex token" >&2
  exit 1
fi
if [ -n "$identity_platform_token" ] && ! valid_token "$identity_platform_token"; then
  echo "error: ${identity_platform_key} must be a 64-character hex token" >&2
  exit 1
fi
if [ -n "$platform_token" ] && ! valid_token "$platform_token"; then
  echo "error: ${platform_token_key} must be a 64-character hex token" >&2
  exit 1
fi
if [ -n "$identity_artifact_token" ] && ! valid_token "$identity_artifact_token"; then
  echo "error: ${identity_artifact_key} must be a 64-character hex token" >&2
  exit 1
fi
if [ -n "$artifact_token" ] && ! valid_token "$artifact_token"; then
  echo "error: ${artifact_token_key} must be a 64-character hex token" >&2
  exit 1
fi
if [ -n "$identity_build_cache_token" ] && ! valid_token "$identity_build_cache_token"; then
  echo "error: ${identity_build_cache_key} must be a 64-character hex token" >&2
  exit 1
fi
if [ -n "$build_cache_token" ] && ! valid_token "$build_cache_token"; then
  echo "error: ${build_cache_token_key} must be a 64-character hex token" >&2
  exit 1
fi
if [ -n "$build_cache_orchestrator_token" ] && ! valid_token "$build_cache_orchestrator_token"; then
  echo "error: ${build_cache_orchestrator_key} must be a 64-character hex token" >&2
  exit 1
fi
if [ -n "$identity_request_token" ] && ! valid_token "$identity_request_token"; then
  echo "error: ${identity_request_key} must be a 64-character hex token" >&2
  exit 1
fi
if [ -n "$platform_request_token" ] && ! valid_token "$platform_request_token"; then
  echo "error: ${platform_request_key} must be a 64-character hex token" >&2
  exit 1
fi
if [ -n "$identity_request_token" ] && [ -n "$platform_request_token" ] && [ "$identity_request_token" != "$platform_request_token" ]; then
  echo "error: Identity and Platform request-verification tokens differ; reconcile them manually" >&2
  exit 1
fi
if [ -n "$identity_token" ] && [ -n "$registry_token" ] && [ "$identity_token" != "$registry_token" ]; then
  echo "error: Identity and Registry authorization tokens differ; reconcile them manually" >&2
  exit 1
fi

service_token="$identity_token"
if [ -z "$service_token" ]; then
  service_token="$registry_token"
fi
if [ -z "$service_token" ]; then
  service_token="$(random_token)"
fi

if [ -n "$identity_platform_token" ] && [ -n "$platform_token" ] && [ "$identity_platform_token" != "$platform_token" ]; then
  echo "error: Identity and Platform admin authorization tokens differ; reconcile them manually" >&2
  exit 1
fi
platform_service_token="$identity_platform_token"
if [ -z "$platform_service_token" ]; then
  platform_service_token="$platform_token"
fi
if [ -z "$platform_service_token" ]; then
  platform_service_token="$(random_token)"
fi
if [ "$service_token" = "$platform_service_token" ]; then
  echo "error: Platform admin authorization token must differ from the Registry token" >&2
  exit 1
fi

if [ -n "$identity_artifact_token" ] && [ -n "$artifact_token" ] && [ "$identity_artifact_token" != "$artifact_token" ]; then
  echo "error: Identity and Artifact admin authorization tokens differ; reconcile them manually" >&2
  exit 1
fi
artifact_service_token="$identity_artifact_token"
if [ -z "$artifact_service_token" ]; then
  artifact_service_token="$artifact_token"
fi
if [ -z "$artifact_service_token" ]; then
  artifact_service_token="$(random_token)"
fi
if [ "$artifact_service_token" = "$service_token" ] || [ "$artifact_service_token" = "$platform_service_token" ]; then
  echo "error: Artifact admin authorization token must differ from Registry and Platform tokens" >&2
  exit 1
fi

if [ -n "$identity_build_cache_token" ] && [ -n "$build_cache_token" ] && [ "$identity_build_cache_token" != "$build_cache_token" ]; then
  echo "error: Identity and Build Cache admin authorization tokens differ; reconcile them manually" >&2
  exit 1
fi
build_cache_service_token="$identity_build_cache_token"
if [ -z "$build_cache_service_token" ]; then build_cache_service_token="$build_cache_token"; fi
if [ -z "$build_cache_service_token" ]; then build_cache_service_token="$(random_token)"; fi
if [ "$build_cache_service_token" = "$service_token" ] || [ "$build_cache_service_token" = "$platform_service_token" ] || [ "$build_cache_service_token" = "$artifact_service_token" ]; then
  echo "error: Build Cache admin authorization token must be unique" >&2
  exit 1
fi
if [ -z "$build_cache_orchestrator_token" ]; then build_cache_orchestrator_token="$(random_token)"; fi
if [ "$build_cache_orchestrator_token" = "$service_token" ] || [ "$build_cache_orchestrator_token" = "$platform_service_token" ] || [ "$build_cache_orchestrator_token" = "$artifact_service_token" ] || [ "$build_cache_orchestrator_token" = "$build_cache_service_token" ]; then
  echo "error: Build Cache orchestrator token must differ from all service authorization tokens" >&2
  exit 1
fi
platform_request_service_token="$identity_request_token"
if [ -z "$platform_request_service_token" ]; then platform_request_service_token="$platform_request_token"; fi
if [ -z "$platform_request_service_token" ]; then platform_request_service_token="$(random_token)"; fi
for existing_token in "$service_token" "$platform_service_token" "$artifact_service_token" "$build_cache_service_token" "$build_cache_orchestrator_token"; do
  if [ "$platform_request_service_token" = "$existing_token" ]; then
    echo "error: Platform request-verification token must be unique" >&2
    exit 1
  fi
done

write_env_value() {
  local file="$1"
  local key="$2"
  local value="$3"
  local temp_file
  temp_file="$(mktemp "${file}.XXXXXX")"
  awk -v key="$key" -v value="$value" '
    BEGIN { prefix = key "=" }
    index($0, prefix) == 1 { print prefix value; found = 1; next }
    { print }
    END { if (!found) print prefix value }
  ' "$file" > "$temp_file"
  chown "root:${VAPOR_GROUP}" "$temp_file"
  chmod 0640 "$temp_file"
  mv -f "$temp_file" "$file"
}

write_env_value "$identity_env" "$identity_key" "$service_token"
write_env_value "$registry_env" "$registry_key" "$service_token"
write_env_value "$identity_env" "$identity_platform_key" "$platform_service_token"
write_env_value "$platform_env" "$platform_token_key" "$platform_service_token"
write_env_value "$identity_env" "$identity_artifact_key" "$artifact_service_token"
write_env_value "$artifact_env" "$artifact_token_key" "$artifact_service_token"
write_env_value "$identity_env" "$identity_build_cache_key" "$build_cache_service_token"
write_env_value "$build_cache_env" "$build_cache_token_key" "$build_cache_service_token"
write_env_value "$build_cache_env" "$build_cache_orchestrator_key" "$build_cache_orchestrator_token"
write_env_value "$identity_env" "$identity_request_key" "$platform_request_service_token"
write_env_value "$platform_env" "$platform_request_key" "$platform_request_service_token"

authz_url="http://127.0.0.1:7113/v1/internal/authorization/check"
write_env_value "$registry_env" "VAPOR_REGISTRY_IDENTITY_AUTHZ_URL" "$authz_url"
write_env_value "$platform_env" "VAPOR_PLATFORM_IDENTITY_AUTHZ_URL" "$authz_url"
write_env_value "$artifact_env" "VAPOR_ARTIFACT_IDENTITY_AUTHZ_URL" "$authz_url"
write_env_value "$build_cache_env" "VAPOR_BUILD_CACHE_IDENTITY_AUTHZ_URL" "$authz_url"
request_authz_url="http://127.0.0.1:7113/v1/internal/automation/verify"
write_env_value "$platform_env" "VAPOR_PLATFORM_IDENTITY_REQUEST_AUTHZ_URL" "$request_authz_url"

echo "authorization: Registry, Platform, Artifact, and Build Cache use separate server-local Identity tokens; values hidden"
