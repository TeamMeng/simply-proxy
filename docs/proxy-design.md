# SimplyProxy 设计文档

## 概述

SimplyProxy 是一个基于 [Pingora](https://github.com/cloudflare/pingora) 构建的简单 HTTP 反向代理，支持多域名路由、负载均衡、健康检查和 TLS。

## 架构

```
Client
  │
  ▼
SimplyProxy (0.0.0.0:8080)
  │
  ├── RouteTable（域名 → RouteEntry 映射）
  │     ├── api.acme.com → LoadBalancer [3001, 3002] (RoundRobin)
  │     ├── www.acme.com → LoadBalancer [3003, 3004] (RoundRobin)
  │     └── acme.com     → LoadBalancer [3005]       (RoundRobin)
  │
  └── HealthService（后台定时检查各 backend 可用性）
```

## 模块结构

```
src/
├── lib.rs                 # 公共导出
├── main.rs                # 入口，组装服务
├── conf/                  # 配置解析
├── proxy/
│   ├── mod.rs             # 核心结构体定义（SimplyProxy、RouteTable、RouteEntry、HealthService）
│   ├── simple_proxy.rs    # ProxyHttp trait 实现
│   ├── route.rs           # 路由表构建与 backend 选择
│   └── health.rs          # 后台健康检查服务
└── utils.rs               # get_host_port 等工具函数
```

## 路由

### RouteTable

`RouteTable` 是一个线程安全的 `HashMap<String, RouteEntry>`（基于 [papaya](https://github.com/ibraheemdev/papaya)），key 为域名，value 为对应的 `RouteEntry`。

启动时由 `ProxyConfigResolved` 构建，每个域名对应一个 `LoadBalancer<RoundRobin>`。

### RouteEntry

每个 `RouteEntry` 包含：

| 字段       | 类型                          | 说明                      |
| ---------- | ----------------------------- | ------------------------- |
| `upstream` | `Arc<LoadBalancer<RoundRobin>>` | 该域名的后端负载均衡器    |
| `tls`      | `bool`                        | 连接上游时是否使用 TLS    |

### backend 选择

`RouteEntry::select()` 使用 `select_with` 只返回健康的 backend：

```rust
pub fn select(&self) -> Option<Backend> {
    let accept = |b: &Backend, health: bool| health;
    self.upstream.select_with(b"", 32, accept)
}
```

返回 `None` 时，代理响应 **502 Bad Gateway**；域名未在路由表中时响应 **404 Not Found**。

## 健康检查

`HealthService` 作为独立 Pingora Service 运行（单线程），每 **5 秒**轮询所有路由条目：

1. 调用 `upstream.update()` 刷新 backend 列表
2. 调用 `run_health_check(true)` 对每个 backend 发起 TCP 健康检查

每个 `RouteEntry` 内部还配置了间隔 **1 秒**的 `TcpHealthCheck`，两层检查共同维护 backend 的健康状态。

## Header 修改

### 请求 Header（upstream_request_filter）

| Header       | 值                | 作用               |
| ------------ | ----------------- | ------------------ |
| `user-agent` | `SimpleProxy/0.1` | 标识请求来自此代理 |

### 响应 Header（upstream_response_filter）

| Header           | 值                | 作用                                   |
| ---------------- | ----------------- | -------------------------------------- |
| `x-simple-proxy` | `v0.1`            | 标识响应经过了此代理                   |
| `server`         | `SimpleProxy/0.1` | 隐藏上游服务器身份（仅在上游未设置时） |

## 工具函数

### get_host_port

从请求中解析出 `(host, port)`，优先取 `Host` header，fallback 到 URI：

```rust
pub(crate) fn get_host_port(host: Option<&HeaderValue>, uri: &Uri) -> (&str, u16)
```

默认端口：HTTPS → 443，其他 → 80。

## 错误处理

| 场景                        | 响应码 |
| --------------------------- | ------ |
| 域名不在路由表              | 404    |
| 所有 backend 均不健康       | 502    |
| 连接上游失败                | 502    |
| 下游读写错误/连接已关闭     | 不响应 |
| 其他内部错误                | 500    |

## 设计决策

### 为什么用 papaya HashMap？

路由表在请求处理热路径上需要无锁并发读，papaya 提供了基于 epoch 的无锁并发 HashMap，读操作不需要加锁。

### 为什么修改 header？

- **user-agent**：让上游服务能够区分代理流量和直连流量
- **x-simple-proxy**：让客户端能够验证请求是否经过了代理
- **server**：安全措施，隐藏上游服务器的软件和版本信息

### 为什么要保留上游的 server header？

代理只会在上游没有设置 `server` header 时才使用默认值。如果上游已经设置了 `server` header，保留它而不是覆盖。

```rust
if !upstream_response.headers.contains_key("server") {
    upstream_response.insert_header("server", "SimpleProxy/0.1")?;
}
```
