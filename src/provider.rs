//! Provider profiles, safe endpoint validation, and explicit genai service targets.
use crate::{config::Provider, credentials};
use anyhow::{Context, Result, bail};
use genai::{
    Client, ModelIden, ServiceTarget,
    adapter::AdapterKind,
    resolver::{AuthData, Endpoint},
};
use secrecy::ExposeSecret;
use url::{Host, Url};

fn adapter(provider: &Provider) -> Result<AdapterKind> {
    match provider.protocol.as_str() {
        "openai" | "openai_compatible" => Ok(AdapterKind::OpenAI),
        "anthropic" => Ok(AdapterKind::Anthropic),
        "gemini" => Ok(AdapterKind::Gemini),
        "openrouter" => Ok(AdapterKind::OpenRouter),
        _ => bail!("Unsupported provider protocol"),
    }
}

pub fn normalize_endpoint(value: &str) -> Result<String> {
    let mut url = Url::parse(value).context("Provider endpoint must be an absolute URL")?;
    if url.username() != "" || url.password().is_some() {
        bail!("Provider endpoint must not contain credentials");
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("Provider endpoint must not contain a query or fragment");
    }
    let host = url.host().context("Provider endpoint has no host")?;
    let local = match host {
        Host::Domain(name) => name.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(address) => address.is_loopback(),
        Host::Ipv6(address) => address.is_loopback(),
    };
    match url.scheme() {
        "https" => {}
        "http" if local => {}
        "http" => bail!("Plain HTTP is allowed only for loopback endpoints"),
        _ => bail!("Provider endpoint must use HTTPS, or HTTP on loopback"),
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url.into())
}

pub fn target(provider: &Provider, model: &str) -> Result<ServiceTarget> {
    crate::config::validate_provider_model(model)?;
    let endpoint = Endpoint::from_owned(normalize_endpoint(&provider.base_url)?);
    let secret = credentials::resolve(&provider.credential)?;
    let auth = AuthData::from_single(secret.expose_secret().to_owned());
    Ok(ServiceTarget {
        model: ModelIden::new(adapter(provider)?, model),
        auth,
        endpoint,
    })
}

pub async fn discover(client: &Client, provider: &Provider) -> Result<Vec<String>> {
    let endpoint = Endpoint::from_owned(normalize_endpoint(&provider.base_url)?);
    let secret = credentials::resolve(&provider.credential)?;
    let auth = AuthData::from_single(secret.expose_secret().to_owned());
    let mut models = client
        .all_model_names(adapter(provider)?, (endpoint, auth))
        .await
        .context("Model discovery failed")?;
    models.retain(|model| crate::config::validate_provider_model(model).is_ok());
    models.sort();
    models.dedup();
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_normalized_and_unsafe_urls_rejected() {
        assert_eq!(
            normalize_endpoint("https://example.com/v1").unwrap(),
            "https://example.com/v1/"
        );
        assert!(normalize_endpoint("https://key@example.com/v1").is_err());
        assert!(normalize_endpoint("http://example.com/v1").is_err());
        assert!(normalize_endpoint("http://127.0.0.1:8000/v1").is_ok());
        assert!(normalize_endpoint("https://example.com/v1?key=secret").is_err());
    }
}
