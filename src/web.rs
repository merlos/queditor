//! Permission-gated, bounded public-web GETs. No cookies, credentials, proxies or redirects.
use crate::tools::MAX_CONTEXT_BYTES;
use anyhow::{Context, Result, bail};
use std::{net::IpAddr, time::Duration};
use url::{Host, Url};

const TIMEOUT: Duration = Duration::from_secs(30);

/// Pure validation: do not perform DNS or network I/O before permission.
pub fn validate_url(requested: &str) -> Result<Url> {
    if requested.len() > 8192 || requested.chars().any(char::is_control) {
        bail!("Web URL is too long or contains control characters");
    }
    let url = Url::parse(requested).map_err(|_| anyhow::anyhow!("Invalid web URL"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        bail!("Web fetching requires an absolute HTTP(S) URL");
    }
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        bail!("Web URLs must not contain credentials or fragments");
    }
    match url.host().context("Web URL has no host")? {
        Host::Ipv4(ip) if !public_ip(ip.into()) => {
            bail!("Private/non-public web addresses are prohibited")
        }
        Host::Ipv6(ip) if !public_ip(ip.into()) => {
            bail!("Private/non-public web addresses are prohibited")
        }
        Host::Domain(name)
            if name == "localhost" || name.ends_with(".localhost") || !name.contains('.') =>
        {
            bail!("Local web hosts are prohibited");
        }
        _ => {}
    }
    Ok(url)
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_documentation()
                && a != 0
                && a < 224
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 198 && matches!(b, 18 | 19))
                && !(a == 192 && b == 0 && c == 0)
                && !(a == 192 && b == 88 && c == 99)
        }
        IpAddr::V6(ip) => {
            let [a, b, ..] = ip.segments();
            // Only global unicast; reject special-purpose and documentation ranges.
            (a & 0xe000) == 0x2000
                && !(a == 0x2001 && b < 0x0200)
                && !(a == 0x2001 && b == 0x0db8)
                && a != 0x2002 // 6to4 can encode a private IPv4 destination.
                && !(a == 0x3fff && b < 0x1000)
        }
    }
}

/// Called only with a capability returned by the local permission decision.
pub(crate) async fn fetch(url: Url) -> Result<String> {
    tokio::time::timeout(TIMEOUT, fetch_inner(url))
        .await
        .map_err(|_| anyhow::anyhow!("Web fetch timed out after 30 seconds"))?
}

async fn fetch_inner(url: Url) -> Result<String> {
    let url = validate_url(url.as_str())?;
    let host = url.host_str().context("Web URL has no host")?;
    let port = url.port_or_known_default().context("Web URL has no port")?;
    let addresses: Vec<_> = match url.host().context("Web URL has no host")? {
        Host::Ipv4(ip) => vec![std::net::SocketAddr::new(ip.into(), port)],
        Host::Ipv6(ip) => vec![std::net::SocketAddr::new(ip.into(), port)],
        Host::Domain(_) => tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| anyhow::anyhow!("Web host lookup failed"))?
            .collect(),
    };
    if addresses.is_empty() || addresses.iter().any(|addr| !public_ip(addr.ip())) {
        bail!("Web host resolves to a private/non-public address");
    }
    // Pin the checked addresses: a second DNS lookup must not bypass validation.
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &addresses)
        .timeout(TIMEOUT)
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("queditor/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| anyhow::anyhow!("Could not initialize web client"))?;
    fetch_text(&client, url).await
}

async fn fetch_text(client: &reqwest::Client, url: Url) -> Result<String> {
    let mut response = client
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            "text/*, application/json, application/xml",
        )
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Web request failed; check network and URL"))?;
    if response.status().is_redirection() {
        bail!(
            "Web redirects are not followed; request permission for the destination URL separately"
        );
    }
    if !response.status().is_success() {
        bail!("Web server returned HTTP {}", response.status().as_u16());
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_CONTEXT_BYTES as u64)
    {
        bail!("Web responses are limited to 256 KiB");
    }
    if let Some(kind) = response.headers().get(reqwest::header::CONTENT_TYPE) {
        let kind = kind
            .to_str()
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !(kind.starts_with("text/")
            || matches!(
                kind.as_str(),
                "application/json" | "application/xml" | "application/xhtml+xml"
            )
            || kind.ends_with("+json")
            || kind.ends_with("+xml"))
        {
            bail!("Web response is not a supported text content type");
        }
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Web response read failed"))?
    {
        if chunk.len() > MAX_CONTEXT_BYTES - bytes.len() {
            bail!("Web responses are limited to 256 KiB");
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.contains(&0) {
        bail!("Web response is binary");
    }
    String::from_utf8(bytes).context("Web responses require UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    #[test]
    fn url_validation_is_local_and_rejects_unsafe_targets() {
        for url in [
            "file:///etc/passwd",
            "ftp://example.com",
            "https://user:secret@example.com",
            "https://example.com/#fragment",
            "http://localhost",
            "http://127.0.0.1",
            "http://2130706433",
            "http://10.0.0.1",
            "http://169.254.169.254",
            "http://[::1]",
            "http://[::ffff:127.0.0.1]",
            "https://example.com/\n",
        ] {
            assert!(validate_url(url).is_err(), "{url}");
        }
        assert_eq!(
            validate_url("https://EXAMPLE.com/info?q=rust")
                .unwrap()
                .as_str(),
            "https://example.com/info?q=rust"
        );
        assert!(validate_url("https://does-not-exist.example/info").is_ok());
    }

    #[test]
    fn rejects_non_public_dns_answers() {
        for ip in [
            "0.0.0.0",
            "127.0.0.1",
            "192.168.1.1",
            "172.16.0.1",
            "100.64.0.1",
            "198.18.0.1",
            "192.0.2.1",
            "224.0.0.1",
            "240.0.0.1",
            "::",
            "fe80::1",
            "fc00::1",
            "2001:db8::1",
            "2002:7f00:1::",
            "3fff::1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
            assert!(public_ip(ip.parse().unwrap()), "{ip}");
        }
    }

    // Only the response parser uses loopback fixtures; the public fetch entry rejects them.
    async fn fixture(status: &str, headers: &str, body: &[u8]) -> Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let response =
            format!("HTTP/1.1 {status}\r\n{headers}\r\nConnection: close\r\n\r\n").into_bytes();
        let body = body.to_vec();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            let n = socket.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..n]).to_ascii_lowercase();
            assert!(!request.contains("authorization:") && !request.contains("cookie:"));
            let _ = socket.write_all(&response);
            let _ = socket.write_all(&body);
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let result = fetch_text(&client, url).await;
        server.join().unwrap();
        result
    }

    #[tokio::test]
    async fn fetches_utf8_html_and_json_without_credentials() {
        assert_eq!(
            fixture(
                "200 OK",
                "Content-Type: text/html; charset=utf-8",
                "<h1>Olá 世界</h1>".as_bytes()
            )
            .await
            .unwrap(),
            "<h1>Olá 世界</h1>"
        );
        assert_eq!(
            fixture("200 OK", "Content-Type: application/json", b"{\"ok\":true}")
                .await
                .unwrap(),
            "{\"ok\":true}"
        );
    }

    #[tokio::test]
    async fn rejects_redirects_errors_binary_and_oversized_bodies() {
        assert!(
            fixture("302 Found", "Location: http://127.0.0.1/secret", b"")
                .await
                .unwrap_err()
                .to_string()
                .contains("redirect")
        );
        let error = fixture(
            "403 Forbidden",
            "Content-Type: text/plain",
            b"SECRET diagnostic",
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(error.contains("403") && !error.contains("SECRET"));
        assert!(
            fixture("200 OK", "Content-Type: image/png", b"PNG")
                .await
                .is_err()
        );
        assert!(
            fixture("200 OK", "Content-Type: text/plain", b"a\0b")
                .await
                .is_err()
        );
        assert!(
            fixture("200 OK", "Content-Type: text/plain", &[0xff])
                .await
                .is_err()
        );
        assert!(
            fixture("200 OK", "Content-Length: 262145", b"")
                .await
                .is_err()
        );
        assert!(
            fixture(
                "200 OK",
                "Content-Type: text/plain",
                &vec![b'a'; MAX_CONTEXT_BYTES + 1]
            )
            .await
            .is_err()
        );
    }
}
