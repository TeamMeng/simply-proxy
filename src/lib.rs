pub mod conf;

use http::StatusCode;
use pingora::{prelude::*, upstreams::peer::Peer};
use tracing::info;

use crate::conf::{ProxyConfig, ProxyConfigResolved};

pub struct SimplyProxy {
    pub(crate) config: ProxyConfig,
}

pub struct ProxyContext {
    pub(crate) config: ProxyConfig,
}

impl SimplyProxy {
    pub fn new(config: ProxyConfigResolved) -> Self {
        Self {
            config: ProxyConfig::new(config),
        }
    }

    pub fn config(&self) -> &ProxyConfig {
        &self.config
    }
}

#[async_trait::async_trait]
impl ProxyHttp for SimplyProxy {
    type CTX = ProxyContext;

    fn new_ctx(&self) -> Self::CTX {
        ProxyContext {
            config: self.config.clone(),
        }
    }

    async fn upstream_peer(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let config = ctx.config.load();

        let Some(host) = session
            .get_header(http::header::HOST)
            .and_then(|h| h.to_str().ok())
            .map(|h| h.split(';').next().unwrap_or(h))
        else {
            return Err(Error::create(
                ErrorType::CustomCode("No valid host found", StatusCode::BAD_GATEWAY.into()),
                ErrorSource::Downstream,
                None,
                None,
            ));
        };

        let Some(server) = config.servers.get(host) else {
            return Err(Error::create(
                ErrorType::HTTPStatus(StatusCode::NOT_FOUND.into()),
                ErrorSource::Upstream,
                None,
                None,
            ));
        };

        let Some(upstream) = server.choose() else {
            return Err(Error::create(
                ErrorType::HTTPStatus(StatusCode::NOT_FOUND.into()),
                ErrorSource::Upstream,
                None,
                None,
            ));
        };

        let mut peer = HttpPeer::new(upstream.to_string(), server.tls, host.to_string());
        if let Some(options) = peer.get_mut_peer_options() {
            options.set_http_version(2, 2);
        }
        info!("upstream peer: {}", peer.to_string());
        Ok(Box::new(peer))
    }

    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        upstream_request: &mut RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()>
    where
        Self::CTX: Send + Sync,
    {
        info!("upstream request filtered: {:?}", upstream_request);
        upstream_request.insert_header("user-agent", "SimpleProxy/0.1")?;
        Ok(())
    }

    async fn upstream_response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()>
    where
        Self::CTX: Send + Sync,
    {
        info!("upstream response filtered: {:?}", upstream_response);
        upstream_response.insert_header("x-simple-proxy", "v0.1")?;

        if !upstream_response.headers.contains_key("server") {
            upstream_response.insert_header("server", "SimpleProxy/0.1")?;
        }

        Ok(())
    }
}
