use axum::http::StatusCode;
use jsonwebtoken::jwk::{Jwk, JwkSet, KeyAlgorithm};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use reqwest::Client;
use serde::Deserialize;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const EXPECTED_REPOSITORY: &str = "GHF-Studios/Vapor-Platform-Server";
const EXPECTED_REPOSITORY_ID: &str = "1309950970";
const EXPECTED_OWNER_ID: &str = "116163865";
const EXPECTED_WORKFLOW: &str =
    "GHF-Studios/Vapor-Platform-Server/.github/workflows/deploy.yml@refs/heads/main";
const EXPECTED_ISSUER: &str = "https://token.actions.githubusercontent.com";
const EXPECTED_AUDIENCE: &str = "vapor-platform-deploy-request";
const MAX_TOKEN_BYTES: usize = 16 * 1024;
const MAX_JWKS_BYTES: usize = 64 * 1024;
const MAX_TOKEN_AGE_SECONDS: i64 = 10 * 60;
const JWKS_CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const UNKNOWN_KEY_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
static JWKS_CACHE: OnceLock<tokio::sync::Mutex<Option<(Instant, JwkSet)>>> = OnceLock::new();

#[derive(Debug, Deserialize)]
pub(crate) struct GithubActionsClaims {
    pub(crate) repository: String,
    repository_id: String,
    repository_owner_id: String,
    #[serde(rename = "ref")]
    pub(crate) git_ref: String,
    pub(crate) sha: String,
    pub(crate) workflow_ref: String,
    pub(crate) run_id: String,
    pub(crate) run_attempt: String,
    pub(crate) event_name: String,
    #[serde(rename = "iat")]
    issued_at: i64,
    #[serde(rename = "exp")]
    expires_at: i64,
}

pub(crate) async fn verify_deploy_request_token(
    client: &Client,
    token: &str,
) -> Result<GithubActionsClaims, StatusCode> {
    if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let header = decode_header(token).map_err(|_| StatusCode::UNAUTHORIZED)?;
    if header.alg != Algorithm::RS256 {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let key_id = header.kid.ok_or(StatusCode::UNAUTHORIZED)?;
    let jwk = signing_key(client, &key_id).await?;
    if jwk.common.key_algorithm != Some(KeyAlgorithm::RS256)
        || jwk
            .common
            .public_key_use
            .as_ref()
            .is_some_and(|use_type| use_type != &jsonwebtoken::jwk::PublicKeyUse::Signature)
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    decode_and_validate(token, &jwk).map_err(|_| StatusCode::UNAUTHORIZED)
}

async fn signing_key(client: &Client, key_id: &str) -> Result<Jwk, StatusCode> {
    let cache = JWKS_CACHE.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut cache = cache.lock().await;
    if let Some((fetched_at, jwks)) = cache.as_ref() {
        if fetched_at.elapsed() < JWKS_CACHE_TTL {
            if let Some(jwk) = jwks.find(key_id) {
                return Ok(jwk.clone());
            }
            if fetched_at.elapsed() < UNKNOWN_KEY_REFRESH_INTERVAL {
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
    }

    let response = client
        .get("https://token.actions.githubusercontent.com/.well-known/jwks")
        .send()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if !response.status().is_success() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let jwks_bytes = read_bounded(response, MAX_JWKS_BYTES)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let jwks: JwkSet =
        serde_json::from_slice(&jwks_bytes).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let jwk = jwks.find(key_id).cloned().ok_or(StatusCode::UNAUTHORIZED)?;
    *cache = Some((Instant::now(), jwks));
    Ok(jwk)
}

async fn read_bounded(response: reqwest::Response, maximum: usize) -> Result<Vec<u8>, ()> {
    if response
        .content_length()
        .is_some_and(|length| length > maximum as u64)
    {
        return Err(());
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
        if bytes.len().saturating_add(chunk.len()) > maximum {
            return Err(());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn decode_and_validate(token: &str, jwk: &Jwk) -> Result<GithubActionsClaims, ()> {
    let decoding_key = DecodingKey::from_jwk(jwk).map_err(|_| ())?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[EXPECTED_ISSUER]);
    validation.set_audience(&[EXPECTED_AUDIENCE]);
    validation.set_required_spec_claims(&["exp", "iat", "iss", "aud"]);
    let claims = decode::<GithubActionsClaims>(token, &decoding_key, &validation)
        .map_err(|_| ())?
        .claims;
    validate_claims(&claims, unix_now())?;
    Ok(claims)
}

fn validate_claims(claims: &GithubActionsClaims, now: i64) -> Result<(), ()> {
    // `workflow_ref` pins the authorized workflow path and ref. The run's
    // `sha` separately binds the request to the deployed source revision;
    // GitHub's `workflow_sha` can legitimately refer to an older workflow
    // definition revision, so it must not be equated with the run's `sha`.
    if claims.repository != EXPECTED_REPOSITORY
        || claims.repository_id != EXPECTED_REPOSITORY_ID
        || claims.repository_owner_id != EXPECTED_OWNER_ID
        || claims.git_ref != "refs/heads/main"
        || claims.workflow_ref != EXPECTED_WORKFLOW
        || !matches!(claims.event_name.as_str(), "push" | "workflow_dispatch")
        || claims.sha.len() != 40
        || !claims.sha.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !claims.run_id.parse::<u64>().is_ok_and(|id| id > 0)
        || !claims
            .run_attempt
            .parse::<u32>()
            .is_ok_and(|attempt| attempt > 0)
        || claims.issued_at > now + 60
        || claims.expires_at <= now - 60
        || now.saturating_sub(claims.issued_at) > MAX_TOKEN_AGE_SECONDS
    {
        return Err(());
    }
    Ok(())
}

pub(crate) fn matches_deploy_request(
    claims: &GithubActionsClaims,
    repository: &str,
    branch: &str,
    commit: &str,
    workflow_url: &str,
) -> bool {
    repository == EXPECTED_REPOSITORY
        && branch == "main"
        && commit == claims.sha
        && workflow_url
            == format!(
                "https://github.com/{EXPECTED_REPOSITORY}/actions/runs/{}",
                claims.run_id
            )
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{validate_claims, GithubActionsClaims};

    fn claims() -> GithubActionsClaims {
        GithubActionsClaims {
            repository: "GHF-Studios/Vapor-Platform-Server".to_owned(),
            repository_id: "1309950970".to_owned(),
            repository_owner_id: "116163865".to_owned(),
            git_ref: "refs/heads/main".to_owned(),
            sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            workflow_ref:
                "GHF-Studios/Vapor-Platform-Server/.github/workflows/deploy.yml@refs/heads/main"
                    .to_owned(),
            run_id: "123456".to_owned(),
            run_attempt: "1".to_owned(),
            event_name: "push".to_owned(),
            issued_at: 1_800_000_000,
            expires_at: 1_800_000_300,
        }
    }

    #[test]
    fn accepts_only_a_fresh_main_deployment_workflow_claim_set() {
        assert!(validate_claims(&claims(), 1_800_000_120).is_ok());
    }

    #[test]
    fn rejects_a_different_repository_branch_or_workflow() {
        let mut value = claims();
        value.repository = "someone/else".to_owned();
        assert!(validate_claims(&value, 1_800_000_120).is_err());

        let mut value = claims();
        value.git_ref = "refs/heads/feature".to_owned();
        assert!(validate_claims(&value, 1_800_000_120).is_err());

        let mut value = claims();
        value.repository_id = "1".to_owned();
        assert!(validate_claims(&value, 1_800_000_120).is_err());

        let mut value = claims();
        value.workflow_ref =
            "GHF-Studios/Vapor-Platform-Server/.github/workflows/other.yml@refs/heads/main"
                .to_owned();
        assert!(validate_claims(&value, 1_800_000_120).is_err());
    }

    #[test]
    fn rejects_old_or_invalid_workflow_runs() {
        let mut value = claims();
        value.event_name = "pull_request".to_owned();
        assert!(validate_claims(&value, 1_800_000_120).is_err());

        let mut value = claims();
        value.issued_at = 1_799_999_000;
        assert!(validate_claims(&value, 1_800_000_120).is_err());

        let mut value = claims();
        value.sha = "not-a-commit".to_owned();
        assert!(validate_claims(&value, 1_800_000_120).is_err());
    }

    #[test]
    fn request_body_must_match_the_signed_repository_revision_and_run() {
        let value = claims();
        let url = format!(
            "https://github.com/GHF-Studios/Vapor-Platform-Server/actions/runs/{}",
            value.run_id
        );
        assert!(super::matches_deploy_request(
            &value,
            "GHF-Studios/Vapor-Platform-Server",
            "main",
            &value.sha,
            &url,
        ));
        assert!(!super::matches_deploy_request(
            &value,
            "GHF-Studios/Vapor-Platform-Server",
            "main",
            "1123456789abcdef0123456789abcdef01234567",
            &url,
        ));
        assert!(!super::matches_deploy_request(
            &value,
            "GHF-Studios/Vapor-Platform-Server",
            "feature",
            &value.sha,
            &url,
        ));
    }
}
