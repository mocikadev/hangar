//! 共享 ureq Agent 工厂。
//!
//! 公司网络等环境下 IPv6 出口可能被重置（RST），而 ureq 2.x 的 std resolver
//! 直接采用首个解析结果、无 happy-eyeballs 回退，导致先命中 AAAA 记录时
//! 请求整体失败（App 的 URLSession 会自动回退 IPv4 所以不受影响）。
//! 这里统一优先 IPv4：仅当域名没有 A 记录时才回退 AAAA。

use std::net::IpAddr;
use std::time::Duration;

/// 优先 IPv4 的 resolver：A 记录存在则只用 IPv4，否则回退 IPv6。
#[derive(Debug)]
pub struct Ipv4FirstResolver;

impl ureq::Resolver for Ipv4FirstResolver {
    fn resolve(&self, netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
        use std::net::ToSocketAddrs;
        let addrs: Vec<std::net::SocketAddr> = netloc.to_socket_addrs()?.collect();
        let v4: Vec<_> = addrs.iter().filter(|a| a.is_ipv4()).copied().collect();
        if v4.is_empty() {
            Ok(addrs)
        } else {
            Ok(v4)
        }
    }
}

/// 统一的 Agent 构造：连接超时 10s / 总超时 25s（与既有各处一致）。
pub fn agent() -> ureq::Agent {
    build(Duration::from_secs(10), Duration::from_secs(25))
}

/// 自定义超时的 Agent 构造。
pub fn build(connect: Duration, total: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(connect)
        .timeout(total)
        .resolver(Ipv4FirstResolver)
        .build()
}

/// 测试辅助：判断一组地址是否会被该 resolver 收敛为仅 IPv4。
pub fn prefer_ipv4(addrs: &[IpAddr]) -> Vec<IpAddr> {
    let v4: Vec<_> = addrs.iter().filter(|a| a.is_ipv4()).copied().collect();
    if v4.is_empty() {
        addrs.to_vec()
    } else {
        v4
    }
}
