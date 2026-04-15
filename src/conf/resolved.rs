//! 解析后配置层：将名称引用解析为实际内容
//!
//! 此模块定义配置的第二阶段——将 raw.rs 中的"名称引用"解析为实际内容。
//!
//! # 转换流程
//! 1. 从 YAML 加载 `SimpleProxyConfig`（使用路径和名称引用）
//! 2. 将证书文件路径加载为实际证书内容
//! 3. 建立 HashMap 通过名称快速查找证书和 upstream
//! 4. 解析所有 server 的 tls/upstream 引用，验证引用完整性
//! 5. 输出最终的 `ProxyConfigResolved`，供运行时直接使用
//!
//! # 错误处理
//! 所有配置错误在启动时暴露：
//! - 证书文件不存在或无法读取
//! - 引用了不存在的证书名称
//! - 引用了不存在的 upstream 名称
//! - 域名重复配置

use crate::conf::raw::{CertConfig, GlobalConfig, ServerConfig, SimpleProxyConfig, UpstreamConfig};
use anyhow::{Context, Result, anyhow};
use rand::rng;
use rand::seq::IndexedRandom;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs;

/// 解析后的完整代理配置
///
/// 包含全局配置和按域名索引的服务器配置 map
#[derive(Debug, Clone)]
pub struct ProxyConfigResolved {
    /// 解析后的全局配置
    pub global: GlobalConfigResolved,
    /// 按域名索引的服务器配置（一个域名对应一个 ServerConfigResolved）
    pub servers: HashMap<String, ServerConfigResolved>,
}

/// 解析后的全局配置
///
/// 将 tls 名称引用替换为实际的证书内容
#[derive(Debug, Clone)]
pub struct GlobalConfigResolved {
    /// 代理监听端口
    pub port: u16,
    /// 解析后的 TLS 证书内容（可选）
    pub tls: Option<CertConfigResolved>,
}

/// 解析后的证书配置
///
/// 包含实际的 PEM 格式证书和私钥内容，可直接用于 TLS 连接
#[derive(Debug, Clone)]
pub struct CertConfigResolved {
    /// PEM 格式证书内容
    pub cert: String,
    /// PEM 格式私钥内容
    pub key: String,
}

/// 解析后的服务器配置
#[derive(Debug, Clone)]
pub struct ServerConfigResolved {
    /// 服务器的 TLS 证书（可选）
    pub tls: Option<CertConfigResolved>,
    /// 解析后的上游服务器配置
    pub upstream: UpstreamConfigResolved,
}

/// 解析后的上游服务器配置
///
/// 包含实际的上游服务器地址列表
#[derive(Debug, Clone)]
pub struct UpstreamConfigResolved {
    /// 上游服务器地址列表
    pub servers: Vec<String>,
}

// ==================== TryFrom 实现 ====================

/// 从 CertConfig（文件路径引用）转换为 CertConfigResolved（实际内容）
impl TryFrom<&CertConfig> for CertConfigResolved {
    type Error = anyhow::Error;

    fn try_from(cert: &CertConfig) -> Result<Self, Self::Error> {
        // 读取证书文件内容
        let cert_content = fs::read_to_string(&cert.cert_path)
            .with_context(|| format!("Failed to load certificate from: {:?}", cert.cert_path))?;

        // 读取私钥文件内容
        let key_content = fs::read_to_string(&cert.key_path)
            .with_context(|| format!("Failed to load key from: {:?}", cert.key_path))?;

        Ok(CertConfigResolved {
            cert: cert_content,
            key: key_content,
        })
    }
}

/// 从 UpstreamConfig 转换为 UpstreamConfigResolved
///
/// 由于 upstream 使用的是实际地址字符串，无需文件加载，直接复制
impl From<&UpstreamConfig> for UpstreamConfigResolved {
    fn from(upstream: &UpstreamConfig) -> Self {
        UpstreamConfigResolved {
            servers: upstream.servers.clone(),
        }
    }
}

/// 从 SimpleProxyConfig 转换为 ProxyConfigResolved
///
/// 执行完整的配置解析流程：
/// 1. 加载所有证书文件
/// 2. 构建证书和 upstream 的名称索引
/// 3. 解析全局配置中的证书引用
/// 4. 解析每个服务器的证书和 upstream 引用
/// 5. 验证域名唯一性
impl TryFrom<SimpleProxyConfig> for ProxyConfigResolved {
    type Error = anyhow::Error;

    fn try_from(raw: SimpleProxyConfig) -> Result<Self, Self::Error> {
        // 步骤1: 加载所有证书，建立名称 -> 内容 的索引
        let mut cert_map = HashMap::new();
        for cert in &raw.certs {
            let resolved_cert = CertConfigResolved::try_from(cert)?;
            cert_map.insert(cert.name.clone(), resolved_cert);
        }

        // 步骤2: 建立 upstream 名称索引
        let mut upstream_map = HashMap::new();
        for upstream in &raw.upstreams {
            let resolved_upstream = UpstreamConfigResolved::from(upstream);
            upstream_map.insert(upstream.name.clone(), resolved_upstream);
        }

        // 步骤3: 解析全局配置
        let global = GlobalConfigResolved::try_from_with_map(&raw.global, &cert_map)?;

        // 步骤4: 解析每个服务器配置
        let mut servers = HashMap::new();
        for server in raw.servers {
            let resolved_server =
                ServerConfigResolved::try_from_with_maps(&server, &cert_map, &upstream_map)?;

            // 步骤5: 按域名建立索引，检测域名重复
            for server_name in server.server_name {
                if servers.contains_key(&server_name) {
                    return Err(anyhow!("Duplicate server name: {}", server_name));
                }
                servers.insert(server_name, resolved_server.clone());
            }
        }

        Ok(ProxyConfigResolved { global, servers })
    }
}

// ==================== Helper 方法 ====================

/// GlobalConfigResolved 的解析方法
///
/// 根据证书名称从 cert_map 中查找实际的证书内容
impl GlobalConfigResolved {
    fn try_from_with_map(
        global: &GlobalConfig,
        cert_map: &HashMap<String, CertConfigResolved>,
    ) -> Result<Self> {
        let tls = match &global.tls {
            // 如果配置了 tls 名称，查找对应的证书内容
            Some(cert_name) => {
                let cert = cert_map
                    .get(cert_name)
                    .ok_or_else(|| anyhow!("Global TLS certificate '{}' not found", cert_name))?;
                Some(cert.clone())
            }
            None => None,
        };

        Ok(GlobalConfigResolved {
            port: global.port,
            tls,
        })
    }
}

/// ServerConfigResolved 的解析方法
///
/// 根据证书名称和 upstream 名称从对应的 map 中查找实际内容
impl ServerConfigResolved {
    fn try_from_with_maps(
        server: &ServerConfig,
        cert_map: &HashMap<String, CertConfigResolved>,
        upstream_map: &HashMap<String, UpstreamConfigResolved>,
    ) -> Result<Self> {
        // 解析 TLS 证书引用
        let tls = match &server.tls {
            Some(cert_name) => {
                let cert = cert_map
                    .get(cert_name)
                    .ok_or_else(|| anyhow!("Server TLS certificate '{}' not found", cert_name))?;
                Some(cert.clone())
            }
            None => None,
        };

        // 解析 upstream 引用
        let upstream_name = &server.upstream;
        let upstream = upstream_map
            .get(upstream_name)
            .ok_or_else(|| anyhow!("Upstream '{}' not found", upstream_name))?
            .clone();

        Ok(ServerConfigResolved { tls, upstream })
    }

    pub fn choose(&self) -> Option<&str> {
        let mut rng = rng();
        self.upstream.servers.choose(&mut rng).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // Helper to create a temporary file with content
    fn create_temp_file(dir: &TempDir, filename: &str, content: &str) -> PathBuf {
        let file_path = dir.path().join(filename);
        fs::write(&file_path, content).expect("Failed to write temp file");
        file_path
    }

    #[test]
    fn test_cert_config_resolved_try_from() {
        let temp_dir = TempDir::new().unwrap();

        // Create temporary cert and key files
        let cert_content = "-----BEGIN CERTIFICATE-----\nMIICert\n-----END CERTIFICATE-----";
        let key_content = "-----BEGIN PRIVATE KEY-----\nMIIKey\n-----END PRIVATE KEY-----";

        let cert_path = create_temp_file(&temp_dir, "cert.pem", cert_content);
        let key_path = create_temp_file(&temp_dir, "key.pem", key_content);

        // Create raw CertConfig
        let raw_cert = CertConfig {
            name: "test_cert".to_string(),
            cert_path,
            key_path,
        };

        // Convert to resolved
        let resolved_cert = CertConfigResolved::try_from(&raw_cert).unwrap();

        // Verify contents
        assert_eq!(resolved_cert.cert, cert_content);
        assert_eq!(resolved_cert.key, key_content);
    }

    #[test]
    fn test_upstream_config_resolved_from() {
        // Create raw UpstreamConfig
        let raw_upstream = UpstreamConfig {
            name: "test_upstream".to_string(),
            servers: vec!["127.0.0.1:8001".to_string(), "127.0.0.1:8002".to_string()],
        };

        // Convert to resolved
        let resolved_upstream = UpstreamConfigResolved::from(&raw_upstream);

        // Verify contents
        assert_eq!(resolved_upstream.servers.len(), 2);
        assert_eq!(resolved_upstream.servers[0], "127.0.0.1:8001");
        assert_eq!(resolved_upstream.servers[1], "127.0.0.1:8002");
    }

    #[test]
    fn test_global_config_resolved_without_tls() {
        // Create raw GlobalConfig without TLS
        let raw_global = GlobalConfig {
            port: 8080,
            tls: None,
        };

        // Create empty cert map
        let cert_map = HashMap::new();

        // Convert to resolved
        let resolved_global =
            GlobalConfigResolved::try_from_with_map(&raw_global, &cert_map).unwrap();

        // Verify contents
        assert_eq!(resolved_global.port, 8080);
        assert!(resolved_global.tls.is_none());
    }

    #[test]
    fn test_global_config_resolved_with_tls() {
        let temp_dir = TempDir::new().unwrap();

        // Create temporary cert and key files
        let cert_content = "-----BEGIN CERTIFICATE-----\nMIICert\n-----END CERTIFICATE-----";
        let key_content = "-----BEGIN PRIVATE KEY-----\nMIIKey\n-----END PRIVATE KEY-----";

        create_temp_file(&temp_dir, "cert.pem", cert_content);
        create_temp_file(&temp_dir, "key.pem", key_content);

        // Create raw GlobalConfig with TLS
        let raw_global = GlobalConfig {
            port: 8443,
            tls: Some("test_cert".to_string()),
        };

        // Create cert map with our test cert
        let mut cert_map = HashMap::new();
        cert_map.insert(
            "test_cert".to_string(),
            CertConfigResolved {
                cert: cert_content.to_string(),
                key: key_content.to_string(),
            },
        );

        // Convert to resolved
        let resolved_global =
            GlobalConfigResolved::try_from_with_map(&raw_global, &cert_map).unwrap();

        // Verify contents
        assert_eq!(resolved_global.port, 8443);
        assert!(resolved_global.tls.is_some());
        assert_eq!(resolved_global.tls.unwrap().cert, cert_content);
    }

    #[test]
    fn test_server_config_resolved() {
        // Create necessary maps
        let mut cert_map = HashMap::new();
        cert_map.insert(
            "test_cert".to_string(),
            CertConfigResolved {
                cert: "cert_content".to_string(),
                key: "key_content".to_string(),
            },
        );

        let mut upstream_map = HashMap::new();
        upstream_map.insert(
            "test_upstream".to_string(),
            UpstreamConfigResolved {
                servers: vec!["127.0.0.1:8001".to_string(), "127.0.0.1:8002".to_string()],
            },
        );

        // Create raw ServerConfig
        let raw_server = ServerConfig {
            server_name: vec!["example.com".to_string(), "www.example.com".to_string()],
            upstream: "test_upstream".to_string(),
            tls: Some("test_cert".to_string()),
        };

        // Convert to resolved
        let resolved_server =
            ServerConfigResolved::try_from_with_maps(&raw_server, &cert_map, &upstream_map)
                .unwrap();

        // Verify contents
        assert!(resolved_server.tls.is_some());
        assert_eq!(resolved_server.tls.unwrap().cert, "cert_content");
        assert_eq!(resolved_server.upstream.servers.len(), 2);
        assert_eq!(resolved_server.upstream.servers[0], "127.0.0.1:8001");
    }

    #[test]
    fn test_proxy_config_resolved_try_from() {
        let temp_dir = TempDir::new().unwrap();

        // Create temporary cert and key files
        let cert_content = "-----BEGIN CERTIFICATE-----\nMIICert\n-----END CERTIFICATE-----";
        let key_content = "-----BEGIN PRIVATE KEY-----\nMIIKey\n-----END PRIVATE KEY-----";

        let cert_path = create_temp_file(&temp_dir, "cert.pem", cert_content);
        let key_path = create_temp_file(&temp_dir, "key.pem", key_content);

        // Create a complete raw config
        let raw_config = SimpleProxyConfig {
            global: GlobalConfig {
                port: 8443,
                tls: Some("proxy_cert".to_string()),
            },
            certs: vec![
                CertConfig {
                    name: "proxy_cert".to_string(),
                    cert_path: cert_path.clone(),
                    key_path: key_path.clone(),
                },
                CertConfig {
                    name: "web_cert".to_string(),
                    cert_path,
                    key_path,
                },
            ],
            servers: vec![ServerConfig {
                server_name: vec!["example.com".to_string(), "www.example.com".to_string()],
                upstream: "web_servers".to_string(),
                tls: Some("web_cert".to_string()),
            }],
            upstreams: vec![UpstreamConfig {
                name: "web_servers".to_string(),
                servers: vec!["127.0.0.1:8001".to_string(), "127.0.0.1:8002".to_string()],
            }],
        };

        // Convert to resolved
        let resolved_config = ProxyConfigResolved::try_from(raw_config).unwrap();

        // Verify global config
        assert_eq!(resolved_config.global.port, 8443);
        assert!(resolved_config.global.tls.is_some());

        // Verify server configs
        assert!(resolved_config.servers.contains_key("example.com"));
        assert!(resolved_config.servers.contains_key("www.example.com"));

        let server = resolved_config.servers.get("example.com").unwrap();
        assert!(server.tls.is_some());
        assert_eq!(server.upstream.servers.len(), 2);
        assert_eq!(server.upstream.servers[0], "127.0.0.1:8001");
    }

    #[test]
    fn test_error_handling_unknown_cert() {
        // Create a config with a reference to a non-existent certificate
        let raw_config = SimpleProxyConfig {
            global: GlobalConfig {
                port: 8443,
                tls: Some("nonexistent_cert".to_string()),
            },
            certs: vec![],
            servers: vec![],
            upstreams: vec![],
        };

        // Try to convert - should fail
        let result = ProxyConfigResolved::try_from(raw_config);
        assert!(result.is_err());

        // Verify error message mentions the missing certificate
        let error = result.unwrap_err().to_string();
        assert!(error.contains("nonexistent_cert"));
    }

    #[test]
    fn test_error_handling_unknown_upstream() {
        let temp_dir = TempDir::new().unwrap();
        let cert_path = create_temp_file(&temp_dir, "cert.pem", "cert content");
        let key_path = create_temp_file(&temp_dir, "key.pem", "key content");

        // Create a config with a reference to a non-existent upstream
        let raw_config = SimpleProxyConfig {
            global: GlobalConfig {
                port: 8080,
                tls: None,
            },
            certs: vec![CertConfig {
                name: "test_cert".to_string(),
                cert_path,
                key_path,
            }],
            servers: vec![ServerConfig {
                server_name: vec!["example.com".to_string()],
                upstream: "nonexistent_upstream".to_string(),
                tls: None,
            }],
            upstreams: vec![],
        };

        // Try to convert - should fail
        let result = ProxyConfigResolved::try_from(raw_config);
        assert!(result.is_err());

        // Verify error message mentions the missing upstream
        let error = result.unwrap_err().to_string();
        assert!(error.contains("nonexistent_upstream"));
    }

    #[test]
    fn test_error_handling_duplicate_server_name() {
        let temp_dir = TempDir::new().unwrap();
        let cert_path = create_temp_file(&temp_dir, "cert.pem", "cert content");
        let key_path = create_temp_file(&temp_dir, "key.pem", "key content");

        // Create a config with duplicate server names
        let raw_config = SimpleProxyConfig {
            global: GlobalConfig {
                port: 8080,
                tls: None,
            },
            certs: vec![CertConfig {
                name: "test_cert".to_string(),
                cert_path,
                key_path,
            }],
            servers: vec![
                ServerConfig {
                    server_name: vec!["example.com".to_string(), "duplicate.com".to_string()],
                    upstream: "upstream1".to_string(),
                    tls: None,
                },
                ServerConfig {
                    server_name: vec!["other.com".to_string(), "duplicate.com".to_string()],
                    upstream: "upstream1".to_string(),
                    tls: None,
                },
            ],
            upstreams: vec![UpstreamConfig {
                name: "upstream1".to_string(),
                servers: vec!["127.0.0.1:8001".to_string()],
            }],
        };

        // Try to convert - should fail due to duplicate server name
        let result = ProxyConfigResolved::try_from(raw_config);
        assert!(result.is_err());

        // Verify error message mentions the duplicate server name
        let error = result.unwrap_err().to_string();
        assert!(error.contains("duplicate.com"));
    }
}
