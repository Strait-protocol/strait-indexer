//! Webhook subscription management: registration validation and secret
//! generation. The HTTP handlers in `server.rs` are thin wrappers over this.

use rand::RngCore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use strait_store::{CreateWebhookSubscription, Database, WebhookRepo, WebhookSubscriptionRow};

/// Valid values for the `routes` filter — mirrors `TunnelRoute`'s wire strings.
const VALID_ROUTES: &[&str] = &["BTC_TO_HEMI", "HEMI_TO_BTC", "ETH_TO_HEMI", "HEMI_TO_ETH"];
/// Valid values for the `statuses` filter — mirrors `TunnelStatus`'s wire strings.
const VALID_STATUSES: &[&str] = &[
    "INITIATED", "ANCHORED", "PROVING", "FINALIZED", "FAILED", "REORGED",
];

/// Registration request body for `POST /webhooks`.
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub url: String,
    /// Omitted/empty = subscribe to every route (likewise for the other filters).
    pub routes: Option<Vec<String>>,
    pub assets: Option<Vec<String>>,
    pub statuses: Option<Vec<String>>,
}

/// Registration response — the only time the secret and token are ever returned.
#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub id: Uuid,
    pub url: String,
    pub routes: Option<Vec<String>>,
    pub assets: Option<Vec<String>>,
    pub statuses: Option<Vec<String>>,
    /// HMAC-SHA256 key used to sign every delivery (`X-Strait-Signature`).
    /// Store it — it is not retrievable later.
    pub signing_secret: String,
    /// Required in the `X-Management-Token` header to GET/DELETE this
    /// subscription. Store it — it is not retrievable later.
    pub management_token: String,
}

/// Validate a registration request. Returns a human-readable rejection reason.
pub fn validate(req: &RegisterRequest) -> Result<url::ParsedUrl, String> {
    let parsed = url::parse(&req.url)?;

    if let Some(routes) = &req.routes {
        for r in routes {
            if !VALID_ROUTES.contains(&r.as_str()) {
                return Err(format!("unknown route '{r}' (valid: {})", VALID_ROUTES.join(", ")));
            }
        }
    }
    if let Some(statuses) = &req.statuses {
        for s in statuses {
            if !VALID_STATUSES.contains(&s.as_str()) {
                return Err(format!(
                    "unknown status '{s}' (valid: {})",
                    VALID_STATUSES.join(", ")
                ));
            }
        }
    }
    // Assets are open-ended (any ERC-20 symbol), so only sanity-cap the values.
    if let Some(assets) = &req.assets {
        for a in assets {
            if a.is_empty() || a.len() > 32 {
                return Err(format!("invalid asset filter '{a}'"));
            }
        }
    }
    Ok(parsed)
}

/// Register a subscription: generate its secrets and persist it. Assumes the
/// request has already passed [`validate`].
pub async fn register(
    db: &Database,
    req: RegisterRequest,
) -> strait_core::error::Result<RegisterResponse> {
    let signing_secret = random_hex_32();
    let management_token = random_hex_32();

    let row = WebhookRepo::new(db)
        .create(CreateWebhookSubscription {
            url: req.url,
            signing_secret: signing_secret.clone(),
            management_token: management_token.clone(),
            routes: none_if_empty(req.routes),
            assets: none_if_empty(req.assets),
            statuses: none_if_empty(req.statuses),
        })
        .await?;

    Ok(RegisterResponse {
        id: row.id,
        url: row.url,
        routes: row.routes,
        assets: row.assets,
        statuses: row.statuses,
        signing_secret,
        management_token,
    })
}

/// Public (secret-free) view of a subscription, for `GET /webhooks/:id`.
#[derive(Debug, Serialize)]
pub struct SubscriptionView {
    pub id: Uuid,
    pub url: String,
    pub routes: Option<Vec<String>>,
    pub assets: Option<Vec<String>>,
    pub statuses: Option<Vec<String>>,
    pub active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<WebhookSubscriptionRow> for SubscriptionView {
    fn from(r: WebhookSubscriptionRow) -> Self {
        Self {
            id: r.id,
            url: r.url,
            routes: r.routes,
            assets: r.assets,
            statuses: r.statuses,
            active: r.active,
            created_at: r.created_at,
        }
    }
}

fn none_if_empty(v: Option<Vec<String>>) -> Option<Vec<String>> {
    v.filter(|v| !v.is_empty())
}

/// 32 random bytes, hex-encoded (64 chars) — used for both the signing secret
/// and the management token. `OsRng` is a CSPRNG.
fn random_hex_32() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Minimal callback-URL validation without pulling in the `url` crate: require
/// http(s), reject URLs whose host is loopback/private/link-local — the node
/// would otherwise POST server-side to internal addresses on behalf of anyone
/// who can hit the public registration endpoint (SSRF).
mod url {
    /// The pieces of the URL we care about post-validation.
    pub struct ParsedUrl {
        pub host: String,
    }

    pub fn parse(raw: &str) -> Result<ParsedUrl, String> {
        let rest = raw
            .strip_prefix("https://")
            .or_else(|| raw.strip_prefix("http://"))
            .ok_or_else(|| "url must start with http:// or https://".to_string())?;

        // host[:port][/path...]
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        if authority.is_empty() {
            return Err("url has no host".to_string());
        }
        if authority.contains('@') {
            return Err("userinfo in url is not allowed".to_string());
        }
        // Strip :port (handle bracketed IPv6 hosts first).
        let host = if let Some(h) = authority.strip_prefix('[') {
            h.split(']').next().unwrap_or("").to_string()
        } else {
            authority.split(':').next().unwrap_or("").to_string()
        };
        if host.is_empty() {
            return Err("url has no host".to_string());
        }

        if is_forbidden_host(&host) {
            return Err(format!("host '{host}' is not allowed (private/loopback)"));
        }
        Ok(ParsedUrl { host })
    }

    /// Reject loopback, RFC-1918/4193, link-local, and unspecified hosts.
    /// DNS names that *resolve* to private ranges are not caught here — that
    /// would require resolving at validation time and re-checking at delivery
    /// time; acceptable residual risk for a read-only indexer on a PaaS.
    fn is_forbidden_host(host: &str) -> bool {
        let lower = host.to_ascii_lowercase();
        if lower == "localhost" || lower.ends_with(".localhost") || lower.ends_with(".local") {
            return true;
        }
        if let Ok(v4) = lower.parse::<std::net::Ipv4Addr>() {
            return v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast();
        }
        if let Ok(v6) = lower.parse::<std::net::Ipv6Addr>() {
            return v6.is_loopback()
                || v6.is_unspecified()
                // fc00::/7 unique-local + fe80::/10 link-local
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80;
        }
        false
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn accepts_public_https() {
            assert!(parse("https://example.com/hook").is_ok());
            assert!(parse("http://example.com:8080/hook?x=1").is_ok());
        }

        #[test]
        fn rejects_non_http() {
            assert!(parse("ftp://example.com").is_err());
            assert!(parse("example.com").is_err());
        }

        #[test]
        fn rejects_private_hosts() {
            for bad in [
                "http://localhost/h",
                "http://127.0.0.1/h",
                "http://10.0.0.5/h",
                "http://192.168.1.1:9000/h",
                "http://172.16.0.1/h",
                "http://169.254.169.254/latest/meta-data",
                "http://[::1]/h",
                "http://0.0.0.0/h",
            ] {
                assert!(parse(bad).is_err(), "{bad} should be rejected");
            }
        }

        #[test]
        fn rejects_userinfo() {
            assert!(parse("http://user@evil.com/h").is_err());
        }
    }
}
