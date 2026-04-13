use anyhow::Result;
use pingora::{proxy::http_proxy_service, server::Server};
use simply_proxy::SimplyProxy;
use tracing::info;

fn main() -> Result<()> {
    tracing_subscriber::fmt().init();

    let mut server = Server::new(None)?;
    server.bootstrap();

    let sp = SimplyProxy {};

    let proxy_addr = "0.0.0.0:8080";
    let mut proxy = http_proxy_service(&server.configuration, sp);
    proxy.add_tcp(proxy_addr);
    info!("simply proxy is running on {}", proxy_addr);

    server.add_service(proxy);
    server.run_forever();
}

#[cfg(test)]
mod tests {
    #[test]
    fn func() {
        assert_eq!(1, 1);
    }
}
