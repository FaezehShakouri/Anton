//! Bootstrap peer list from ENS (`anton.eth` → text `axl_bootstrap_peers`).

use super::{connect_http, normalize_chat_name, EnsResolverConfig};
use crate::error::{AntonError, Result};

/// Fetch the JSON string array published on `anton.eth` under `axl_bootstrap_peers`.
///
/// Returns an empty list if the record is missing or empty. Malformed JSON is surfaced as an
/// error so operators notice misconfiguration.
pub async fn fetch_axl_bootstrap_peers(
    rpc_url: &str,
    resolver_config: EnsResolverConfig,
) -> Result<Vec<String>> {
    let resolver = connect_http(rpc_url, resolver_config)?;
    let name = normalize_chat_name("anton.eth");
    let raw = match resolver.text_record(&name, "axl_bootstrap_peers").await {
        Ok(s) => s,
        Err(AntonError::EnsResolution(msg))
            if msg.contains("ResolverNotFound")
                || msg.contains("resolver not found")
                || msg.contains("WildcardNotSupported") =>
        {
            return Ok(Vec::new());
        }
        Err(e) => return Err(e),
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let v: Vec<String> = serde_json::from_str(trimmed).map_err(|e| {
        AntonError::EnsResolution(format!("axl_bootstrap_peers JSON parse: {e}"))
    })?;
    Ok(v.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_bootstrap_json_array() {
        let j = r#"["tls://a:9001", "tls://b:9001"]"#;
        let v: Vec<String> = serde_json::from_str(j).unwrap();
        assert_eq!(v.len(), 2);
    }
}
