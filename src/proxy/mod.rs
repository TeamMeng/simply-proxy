mod health;
mod route;
mod simple_proxy;

use bytes::BytesMut;
use papaya::HashMap;
use pingora::{lb::LoadBalancer, prelude::RoundRobin};
use std::sync::Arc;

use crate::conf::ProxyConfig;

pub struct SimpleProxy {
    pub(crate) config: ProxyConfig,
    pub(crate) route_table: RouteTable,
}

#[allow(unused)]
pub struct ProxyContext {
    pub(crate) config: ProxyConfig,
    pub(crate) route_entry: Option<RouteEntry>,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) resp_content_type: Option<String>,
    pub(crate) resp_body: Option<BytesMut>,
}

#[derive(Clone)]
pub struct RouteTable(pub(crate) Arc<HashMap<String, RouteEntry>>);

#[derive(Clone)]
pub struct RouteEntry {
    pub(crate) upstream: Arc<LoadBalancer<RoundRobin>>,
    pub(crate) tls: bool,
}

pub struct HealthService {
    pub(crate) route_table: RouteTable,
}
