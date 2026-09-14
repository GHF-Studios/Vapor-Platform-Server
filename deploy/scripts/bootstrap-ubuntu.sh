install_dir root root 0755 /opt
install_dir root root 0755 "${VAPOR_DEPLOY_ROOT}"

install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/homepage"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/docs"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/identity"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/diagnostics"
install_dir "${VAPOR_USER}" "${VAPOR_GROUP}" 0750 "${VAPOR_STATE_ROOT}/registry"

install_dir root "${VAPOR_GROUP}" 0750 "${VAPOR_CONFIG_DIR}"

chown -R "${VAPOR_USER}:${VAPOR_GROUP}" "${VAPOR_STATE_ROOT}"
find "${VAPOR_STATE_ROOT}" -type d -exec chmod 0750 {} +
find "${VAPOR_STATE_ROOT}" -type f -exec chmod 0640 {} +

install_secret_env "${VAPOR_CONFIG_DIR}/platform.env" \
"VAPOR_DOMAIN=${VAPOR_DOMAIN}
VAPOR_BRANCH=${VAPOR_BRANCH}
# Optional pre-DNS HTTP test host, for example a server IP address:
# VAPOR_HTTP_FALLBACK_HOST="

install_secret_env "${VAPOR_CONFIG_DIR}/homepage.env" \
"VAPOR_HOMEPAGE_BIND=127.0.0.1:7111"

install_secret_env "${VAPOR_CONFIG_DIR}/docs.env" \
"VAPOR_DOCS_BIND=127.0.0.1:7112
VAPOR_DOCS_STATE=${VAPOR_STATE_ROOT}/docs
VAPOR_DOCS_ADMIN_TOKEN=$(random_token)"

install_secret_env "${VAPOR_CONFIG_DIR}/identity.env" \
"VAPOR_IDENTITY_BIND=127.0.0.1:7113
VAPOR_IDENTITY_STATE=${VAPOR_STATE_ROOT}/identity
VAPOR_IDENTITY_DB=${VAPOR_STATE_ROOT}/identity/identity.sqlite3
VAPOR_IDENTITY_ADMIN_TOKEN=$(random_token)
VAPOR_IDENTITY_DASHBOARD_PASSWORD=$(random_token)
VAPOR_IDENTITY_STEAM_APP_ID=2122620
VAPOR_IDENTITY_STEAM_AUTH_IDENTITY=vapor-identity
VAPOR_IDENTITY_COOKIE_PATH=/
# false is only for pre-DNS HTTP-by-IP smoke testing; set true after HTTPS.
VAPOR_IDENTITY_COOKIE_SECURE=false
# Optional stable origin for browser redirects, for example http://82.165.77.104
# before DNS or https://vapor.ghf-studios.site after DNS:
VAPOR_IDENTITY_PUBLIC_ORIGIN=
# Set on the server when available; never commit the value:
VAPOR_IDENTITY_STEAM_WEB_API_KEY=
# Set after creating the GitHub OAuth/GitHub App registration:
VAPOR_IDENTITY_GITHUB_CLIENT_ID=
VAPOR_IDENTITY_GITHUB_CLIENT_SECRET="

install_secret_env "${VAPOR_CONFIG_DIR}/diagnostics.env" \
"VAPOR_DIAGNOSTICS_BIND=127.0.0.1:7114
VAPOR_DIAGNOSTICS_STATE=${VAPOR_STATE_ROOT}/diagnostics
VAPOR_DIAGNOSTICS_MAX_STORED_BYTES=268435456
VAPOR_DIAGNOSTICS_ADMIN_TOKEN=$(random_token)"

install_secret_env "${VAPOR_CONFIG_DIR}/registry.env" \
"VAPOR_REGISTRY_BIND=127.0.0.1:7115
VAPOR_REGISTRY_STATE=${VAPOR_STATE_ROOT}/registry
VAPOR_REGISTRY_DB=${VAPOR_STATE_ROOT}/registry/registry.sqlite3"