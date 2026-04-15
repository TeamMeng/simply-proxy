use anyhow::Result;
use clap::Parser;
use pingora::{proxy::http_proxy_service, server::Server};
use simply_proxy::{SimplyProxy, conf::ProxyConfig};
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = None)]
    config: PathBuf,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().init();

    let args = Args::parse();

    let mut server = Server::new(None)?;
    server.bootstrap();

    let config = ProxyConfig::load(args.config)?;

    let sp = SimplyProxy::new(config);

    let port = sp.config().load().global.port;
    let proxy_addr = format!("0.0.0.0:{}", port);

    let mut proxy = http_proxy_service(&server.configuration, sp);
    proxy.add_tcp(&proxy_addr);
    info!("simply proxy is running on {}", proxy_addr);

    server.add_service(proxy);
    server.run_forever();
}
