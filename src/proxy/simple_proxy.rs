use async_trait::async_trait;
use bytes::Bytes;
use http::{StatusCode, header};
use pingora::{
    cache::{
        CacheKey, CacheMeta, ForcedFreshness, HitHandler, NoCacheReason,
        RespCacheable::{self, Uncacheable},
        key::HashBinary,
    },
    modules::http::{HttpModules, compression::ResponseCompressionBuilder},
    prelude::*,
    protocols::{Digest, http::conditional_filter},
    proxy::{FailToProxy, PurgeStatus},
    upstreams::peer::Peer,
};
use std::time::Duration;
use tracing::{error, info};

use crate::{
    ProxyContext, RouteTable, SimplyProxy,
    conf::{ProxyConfig, ProxyConfigResolved},
    get_host_port,
};

impl SimplyProxy {
    pub fn try_new(config: ProxyConfigResolved) -> anyhow::Result<Self> {
        let route_table = RouteTable::try_new(&config)?;
        Ok(Self {
            config: ProxyConfig::new(config),
            route_table,
        })
    }

    pub fn config(&self) -> &ProxyConfig {
        &self.config
    }

    pub fn route_table(&self) -> &RouteTable {
        &self.route_table
    }
}

#[async_trait]
impl ProxyHttp for SimplyProxy {
    type CTX = ProxyContext;

    fn new_ctx(&self) -> Self::CTX {
        ProxyContext {
            config: self.config.clone(),
            route_entry: None,
            host: "".to_string(),
            port: 80,
        }
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        info!("request_cache_filter");

        let (host, port) =
            get_host_port(session.get_header(header::HOST), &session.req_header().uri);

        let route_table = self.route_table.pin();
        let route_entry = route_table.get(host);
        ctx.route_entry = route_entry.cloned();
        ctx.host = host.to_string();
        ctx.port = port;
        Ok(false)
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let Some(route_entry) = ctx.route_entry.as_ref() else {
            return Err(Error::create(
                ErrorType::HTTPStatus(StatusCode::NOT_FOUND.into()),
                ErrorSource::Upstream,
                None,
                None,
            ));
        };

        let Some(upstream) = route_entry.select() else {
            return Err(Error::create(
                ErrorType::HTTPStatus(StatusCode::BAD_GATEWAY.into()),
                ErrorSource::Upstream,
                None,
                None,
            ));
        };

        let mut peer = HttpPeer::new(upstream, route_entry.tls, ctx.host.clone());
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

    async fn response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()>
    where
        Self::CTX: Send + Sync,
    {
        info!("response_filter: status={}", upstream_response.status);
        Ok(())
    }

    fn init_downstream_modules(&self, modules: &mut HttpModules) {
        info!("init_downstream_modules");
        modules.add_module(ResponseCompressionBuilder::enable(0));
    }

    async fn early_request_filter(&self, _session: &mut Session, _ctx: &mut Self::CTX) -> Result<()>
    where
        Self::CTX: Send + Sync,
    {
        info!("early_request_filter");
        Ok(())
    }

    async fn request_body_filter(
        &self,
        _session: &mut Session,
        _body: &mut Option<Bytes>,
        _end_of_stream: bool,
        _ctx: &mut Self::CTX,
    ) -> Result<()>
    where
        Self::CTX: Send + Sync,
    {
        info!("request_body_filter");
        Ok(())
    }

    fn response_cache_filter(
        &self,
        _session: &Session,
        resp: &ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<RespCacheable> {
        info!("response_cache_filter: status={}", resp.status);
        Ok(Uncacheable(NoCacheReason::Custom("default")))
    }

    fn cache_key_callback(&self, _session: &Session, _ctx: &mut Self::CTX) -> Result<CacheKey> {
        info!("cache_key_callback");
        unimplemented!("cache_key_callback must be implemented when caching is enabled")
    }

    fn cache_miss(&self, session: &mut Session, _ctx: &mut Self::CTX) {
        info!("cache_miss");
        session.cache.cache_miss();
    }

    async fn cache_hit_filter(
        &self,
        _session: &mut Session,
        _meta: &CacheMeta,
        _hit_handler: &mut HitHandler,
        _is_fresh: bool,
        _ctx: &mut Self::CTX,
    ) -> Result<Option<ForcedFreshness>>
    where
        Self::CTX: Send + Sync,
    {
        info!("cache_hit_filter");
        Ok(None)
    }

    async fn proxy_upstream_filter(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<bool>
    where
        Self::CTX: Send + Sync,
    {
        info!("proxy_upstream_filter");
        Ok(true)
    }

    fn cache_not_modified_filter(
        &self,
        session: &Session,
        resp: &ResponseHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<bool> {
        Ok(conditional_filter::not_modified_filter(
            session.req_header(),
            resp,
        ))
    }

    async fn logging(&self, _session: &mut Session, _e: Option<&Error>, _ctx: &mut Self::CTX) {
        info!("logging")
    }

    fn is_purge(&self, _session: &Session, _ctx: &Self::CTX) -> bool {
        info!("is_purge");
        false
    }

    fn purge_response_filter(
        &self,
        _session: &Session,
        _ctx: &mut Self::CTX,
        _purge_status: PurgeStatus,
        _purge_response: &mut std::borrow::Cow<'static, ResponseHeader>,
    ) -> Result<()> {
        info!("purge_response_filter");
        Ok(())
    }

    fn cache_vary_filter(
        &self,
        _meta: &CacheMeta,
        _ctx: &mut Self::CTX,
        _req: &RequestHeader,
    ) -> Option<HashBinary> {
        info!("cache_vary_filter");
        None
    }

    fn upstream_response_body_filter(
        &self,
        _session: &mut Session,
        _body: &mut Option<Bytes>,
        _end_of_stream: bool,
        _ctx: &mut Self::CTX,
    ) -> Result<Option<Duration>> {
        info!("upstream_response_body_filter");
        Ok(None)
    }

    fn upstream_response_trailer_filter(
        &self,
        _session: &mut Session,
        _upstream_trailers: &mut header::HeaderMap,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        info!("upstream_response_trailer_filter");
        Ok(())
    }

    fn response_body_filter(
        &self,
        _session: &mut Session,
        _body: &mut Option<Bytes>,
        _end_of_stream: bool,
        _ctx: &mut Self::CTX,
    ) -> Result<Option<Duration>>
    where
        Self::CTX: Send + Sync,
    {
        info!("response_body_filter");
        Ok(None)
    }

    async fn response_trailer_filter(
        &self,
        _session: &mut Session,
        _upstream_trailers: &mut header::HeaderMap,
        _ctx: &mut Self::CTX,
    ) -> Result<Option<Bytes>>
    where
        Self::CTX: Send + Sync,
    {
        info!("response_trailer_filter");
        Ok(None)
    }

    fn error_while_proxy(
        &self,
        peer: &HttpPeer,
        session: &mut Session,
        e: Box<Error>,
        _ctx: &mut Self::CTX,
        client_reused: bool,
    ) -> Box<Error> {
        info!(
            "error_while_proxy: peer={}, reused={}, error={}",
            peer, client_reused, e
        );
        let mut e = e.more_context(format!("Peer: {}", peer));
        e.retry
            .decide_reuse(client_reused && !session.as_ref().retry_buffer_truncated());
        e
    }

    fn fail_to_connect(
        &self,
        _session: &mut Session,
        peer: &HttpPeer,
        _ctx: &mut Self::CTX,
        e: Box<Error>,
    ) -> Box<Error> {
        info!("fail_to_connect: peer={}, error={}", peer, e);
        e
    }

    async fn fail_to_proxy(
        &self,
        session: &mut Session,
        e: &Error,
        _ctx: &mut Self::CTX,
    ) -> FailToProxy
    where
        Self::CTX: Send + Sync,
    {
        info!("fail_to_proxy: error={}", e);
        let code = match e.etype() {
            HTTPStatus(code) => *code,
            _ => {
                match e.esource() {
                    ErrorSource::Upstream => 502,
                    ErrorSource::Downstream => {
                        match e.etype() {
                            WriteError | ReadError | ConnectionClosed => {
                                /* conn already dead */
                                0
                            }
                            _ => 400,
                        }
                    }
                    ErrorSource::Internal | ErrorSource::Unset => 500,
                }
            }
        };
        if code > 0 {
            session.respond_error(code).await.unwrap_or_else(|e| {
                error!("failed to send error response to downstream: {e}");
            });
        }

        FailToProxy {
            error_code: code,
            can_reuse_downstream: false,
        }
    }

    fn should_serve_stale(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
        error: Option<&Error>,
    ) -> bool {
        error.is_some_and(|e| e.esource() == &ErrorSource::Upstream)
    }

    async fn connected_to_upstream(
        &self,
        _session: &mut Session,
        _reused: bool,
        _peer: &HttpPeer,
        #[cfg(unix)] _fd: std::os::unix::io::RawFd,
        #[cfg(windows)] _sock: std::os::windows::io::RawSocket,
        _digest: Option<&Digest>,
        _ctx: &mut Self::CTX,
    ) -> Result<()>
    where
        Self::CTX: Send + Sync,
    {
        info!("connected_to_upstream");
        Ok(())
    }

    fn request_summary(&self, session: &Session, _ctx: &Self::CTX) -> String {
        info!("request_summary");
        session.as_ref().request_summary()
    }
}
