use pingora::prelude::*;
use tracing::info;

pub struct SimplyProxy {}

#[async_trait::async_trait]
impl ProxyHttp for SimplyProxy {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {}

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let peer = HttpPeer::new("127.0.0.1:3000".to_string(), false, "localhost".to_string());
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
