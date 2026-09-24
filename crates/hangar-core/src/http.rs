//! 共享 ureq Agent 工厂。
//!
//! 公司网络等环境下 IPv6 出口可能被重置（RST），而 ureq 2.x 的 std resolver
//! 直接采用首个解析结果、无 happy-eyeballs 回退，导致先命中 AAAA 记录时
//! 请求整体失败（App 的 URLSession 会自动回退 IPv4 所以不受影响）。
//! 这里统一优先 IPv4，同时保留 IPv6 作为连接失败时的回退。

use std::net::SocketAddr;
use std::time::Duration;

/// 优先 IPv4 的 resolver：保留 IPv6 地址作为连接回退。
#[derive(Debug)]
pub struct Ipv4FirstResolver;

impl ureq::Resolver for Ipv4FirstResolver {
    fn resolve(&self, netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
        use std::net::ToSocketAddrs;
        Ok(order_addresses(netloc.to_socket_addrs()?))
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

fn order_addresses(addrs: impl IntoIterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    let (mut v4, v6): (Vec<_>, Vec<_>) = addrs.into_iter().partition(SocketAddr::is_ipv4);
    v4.extend(v6);
    v4
}

#[cfg(test)]
mod tests {
    use super::order_addresses;
    use std::net::SocketAddr;

    fn addr(value: &str) -> SocketAddr {
        value.parse().unwrap()
    }

    #[test]
    fn mixed_addresses_keep_ipv6_fallback_and_family_order() {
        let original = [
            addr("[::1]:1455"),
            addr("127.0.0.2:1455"),
            addr("[::2]:1455"),
            addr("127.0.0.1:1455"),
        ];
        assert_eq!(
            order_addresses(original),
            [
                addr("127.0.0.2:1455"),
                addr("127.0.0.1:1455"),
                addr("[::1]:1455"),
                addr("[::2]:1455"),
            ]
        );
    }

    #[test]
    fn single_family_addresses_are_unchanged() {
        let v4 = [addr("127.0.0.2:80"), addr("127.0.0.1:80")];
        let v6 = [addr("[::2]:80"), addr("[::1]:80")];
        assert_eq!(order_addresses(v4), v4);
        assert_eq!(order_addresses(v6), v6);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn failed_ipv4_connection_reaches_ipv6_loopback() {
        use std::io::{Read, Write};
        use std::net::{Ipv6Addr, TcpListener};
        use std::thread;
        use std::time::Duration;

        let listener = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            stream.read(&mut request).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        let resolver = move |_: &str| {
            Ok(order_addresses([
                addr(&format!("127.0.0.2:{port}")),
                addr(&format!("[::1]:{port}")),
            ]))
        };
        let result = ureq::AgentBuilder::new()
            .try_proxy_from_env(false)
            .timeout_connect(Duration::from_millis(500))
            .timeout(Duration::from_secs(2))
            .resolver(resolver)
            .build()
            .get(&format!("http://localhost:{port}/"))
            .call()
            .unwrap()
            .into_string()
            .unwrap();
        server.join().unwrap();
        assert_eq!(result, "ok");
    }
}
