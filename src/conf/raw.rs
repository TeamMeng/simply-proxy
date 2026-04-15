//! 原始配置层：从 YAML 文件反序列化得到的配置结构
//!
//! 此模块定义配置的第一阶段——从 YAML 加载后、解析前的原始数据结构。
//! 配置使用"名称引用"而非实际内容（如证书文件路径、upstream 名称），
//! 便于在 YAML 中声明式地组织配置关系。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 主配置结构
///
/// 包含全局配置、证书配置、服务器配置和上游服务器配置。
/// 配置通过名称引用（证书名称、upstream 名称）建立关联关系。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SimpleProxyConfig {
    /// 全局配置设置（端口、TLS 等）
    pub global: GlobalConfig,

    /// 证书配置列表（通过名称引用）
    #[serde(default)]
    pub certs: Vec<CertConfig>,

    /// 服务器配置列表（每个服务器对应一个或多个域名）
    pub servers: Vec<ServerConfig>,

    /// 上游服务器配置列表（负载均衡目标）
    pub upstreams: Vec<UpstreamConfig>,
}

/// 全局配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlobalConfig {
    /// 代理监听的端口
    pub port: u16,

    /// TLS 证书名称引用（可选），指向 certs 列表中的某个证书
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls: Option<String>,
}

/// 证书配置
///
/// 存储证书文件路径，通过名称被 global.tls 或 server.tls 引用
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CertConfig {
    /// 证书名称，用于在配置中引用此证书
    pub name: String,

    /// PEM 格式证书文件的路径
    pub cert_path: PathBuf,

    /// PEM 格式私钥文件的路径
    pub key_path: PathBuf,
}

/// 服务器配置
///
/// 定义一个虚拟服务器，处理一组域名的请求，并转发到指定的上游服务器组
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    /// 此服务器处理的域名列表（可配置多个域名指向同一个 upstream）
    pub server_name: Vec<String>,

    /// 转发目标的上游服务器组名称（引用 upstreams 中的某个 upstream）
    pub upstream: String,

    /// TLS 证书名称引用（可选），指向 certs 列表中的某个证书
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls: Option<String>,
}

/// 上游服务器配置
///
/// 定义一个上游服务器组，包含多个服务器地址，用于负载均衡
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpstreamConfig {
    /// 上游服务器组的名称，用于被 server.upstream 引用
    pub name: String,

    /// 服务器地址列表，格式为 "host:port"
    pub servers: Vec<String>,
}

impl SimpleProxyConfig {
    /// 从 YAML 文件加载配置
    pub fn from_yaml_file(path: impl AsRef<Path>) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        let config = serde_yaml::from_reader(file)?;
        Ok(config)
    }

    /// 从 YAML 字符串加载配置（方便测试）
    pub fn from_yaml_str(yaml: &str) -> Result<Self> {
        let config = serde_yaml::from_str(yaml)?;
        Ok(config)
    }

    /// 将配置保存为 YAML 文件
    pub fn to_yaml_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let file = std::fs::File::create(path)?;
        serde_yaml::to_writer(file, self)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_sample_config() {
        let yaml = include_str!("../../fixtures/sample.yml");
        let config = SimpleProxyConfig::from_yaml_str(yaml).unwrap();

        assert_eq!(config.global.port, 8080);
        assert_eq!(config.global.tls, Some("proxy_cert".to_string()));

        assert_eq!(config.certs.len(), 3);
        assert_eq!(config.certs[0].name, "proxy_cert");

        assert_eq!(config.servers.len(), 2);
        assert_eq!(
            config.servers[0].server_name,
            vec!["acme.com", "www.acme.com"]
        );
        assert_eq!(config.servers[0].upstream, "web_servers");
        assert_eq!(config.servers[0].tls, Some("web_cert".to_string()));

        assert_eq!(config.upstreams.len(), 2);
        assert_eq!(config.upstreams[0].name, "web_servers");
        assert_eq!(
            config.upstreams[0].servers,
            vec!["127.0.0.1:3001", "127.0.0.1:3002"]
        );
    }
}
