# SimplyProxy 设计文档

## 概述

SimplyProxy 是一个基于 [Pingora](https://github.com/cloudflare/pingora) 构建的简单 HTTP 反向代理。

## 架构

```
Client -> SimplyProxy (0.0.0.0:8080) -> Upstream (127.0.0.1:3000)
```

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

## 设计决策

### 为什么修改 header？

- **user-agent**：让上游服务能够区分代理流量和直连流量
- **x-simple-proxy**：让客户端能够验证请求是否经过了代理
- **server**：安全措施，隐藏上游服务器的软件和版本信息

### 为什么要保留上游的 server header？

代理只会在上游没有设置 `server` header 时才使用默认值。这是刻意的——如果上游已经设置了 `server` header（例如出于合规或品牌原因），我们会保留它而不是覆盖。

```rust
// 仅在上游没有设置 server header 时才设置默认值
if !upstream_response.headers().contains_key("server") {
    upstream_response.insert_header("server", "SimpleProxy/0.1")?;
}
```
