use http::{HeaderValue, Uri};

pub(crate) fn get_host_port<'a>(host: Option<&'a HeaderValue>, uri: &'a Uri) -> (&'a str, u16) {
    let default_port = match uri.scheme() {
        Some(scheme) if scheme.as_str() == "https" => 433,
        _ => 80,
    };

    match host {
        Some(h) => split_host_port(h.to_str().unwrap_or_default(), default_port),
        None => (
            uri.host().unwrap_or_default(),
            uri.port_u16().unwrap_or(default_port),
        ),
    }
}

fn split_host_port(host: &str, default_port: u16) -> (&str, u16) {
    let mut parts = host.split(':');
    let host = parts.next().unwrap_or("");
    let port = parts.next();
    match port {
        Some(port) => (host, port.parse().unwrap_or(default_port)),
        None => (host, default_port),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_host_port() {
        let uri = Uri::from_static("http://localhost:8080");
        let (host, port) = get_host_port(None, &uri);
        assert_eq!(host, "localhost");
        assert_eq!(port, 8080);
    }
}
