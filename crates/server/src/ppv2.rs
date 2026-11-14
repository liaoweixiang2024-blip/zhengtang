//! HAProxy Proxy Protocol v2（https://www.haproxy.org/download/1.8/doc/proxy-protocol.txt）
//!
//! 用途：frp 等 TCP 内网穿透默认把源 IP 丢成 127.0.0.1，frpc 侧开启
//! `transport.proxyProtocolVersion = "v2"` 后，转发流的最前面会带一个 PPv2 头，
//! 写着真实来源地址。监听层在 TLS/HTTP 之前 peek 一下：有头就消费并取出真实
//! 地址（ConnectInfo 全链路受益：last_ip/地区识别/审计 IP），没有则零开销透传。
//!
//! 兼容性：PPv2 签名首字节 0x0D，与 TLS ClientHello（0x16）、明文 HTTP
//! （ASCII）都不同，peek 一字节即可分辨，普通连接不消费任何字节。

use std::io;
use std::net::{IpAddr, SocketAddr};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

const PPV2_SIG: [u8; 12] = *b"\r\n\r\n\x00\r\nQUIT\n";

/// peek 连接开头：是 PPv2 头则消费并返回其中的真实源地址；否则不动任何字节。
///
/// 返回 `Ok(None)` = 普通连接（原样透传）；`Err` = 对端异常（可断开）。
pub async fn maybe_proxied(tcp: &mut TcpStream) -> io::Result<Option<SocketAddr>> {
    // 等至少 1 字节可读（peek 不消费）；首字节不是 0x0D 直接放行
    let mut first = [0u8; 1];
    let n = tcp.peek(&mut first).await?;
    if n == 0 || first[0] != PPV2_SIG[0] {
        return Ok(None);
    }
    // 首字节对上了：等签名 12 字节到齐再整体比对（peek 循环，不消费）
    let mut head = [0u8; 12];
    let mut got = 0;
    while got < 12 {
        let n = tcp.peek(&mut head).await?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "ppv2: 连接在签名中途断开"));
        }
        got = n;
        if head[..12] == PPV2_SIG {
            break;
        }
        // 凑满 12 字节仍不匹配：不是 PPv2（首字节恰好是 \r 的普通流量）
        if n >= 12 {
            return Ok(None);
        }
    }
    if head[..12] != PPV2_SIG {
        return Ok(None);
    }
    // 确认是 PPv2：消费 12 字节签名 + 4 字节（ver/cmd + 头长）
    let mut pre = vec![0u8; 16];
    tcp.read_exact(&mut pre).await?;
    let cmd = pre[12] & 0x0F;
    if cmd == 0x0 {
        // LOCAL：代理自身发起（健康检查等），无源地址可取
        return Ok(None);
    }
    let hlen = u16::from_be_bytes([pre[14], pre[15]]) as usize;
    let fam = pre[13] >> 4;
    let mut payload = vec![0u8; hlen];
    tcp.read_exact(&mut payload).await?;
    parse_addr(fam, &payload)
}

fn parse_addr(fam: u8, p: &[u8]) -> io::Result<Option<SocketAddr>> {
    let addr = match fam {
        // AF_INET：src(4) dst(4) sport(2) dport(2)
        0x1 if p.len() >= 12 => {
            let ip = IpAddr::from([p[0], p[1], p[2], p[3]]);
            SocketAddr::new(ip, u16::from_be_bytes([p[8], p[9]]))
        }
        // AF_INET6：src(16) dst(16) sport(2) dport(2)
        0x2 if p.len() >= 36 => {
            let mut b = [0u8; 16];
            b.copy_from_slice(&p[..16]);
            SocketAddr::new(IpAddr::from(b), u16::from_be_bytes([p[32], p[33]]))
        }
        _ => return Ok(None),
    };
    Ok(Some(addr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ipv4_and_v6() {
        // src 1.2.3.4 sport 443
        let p4 = [1, 2, 3, 4, 10, 0, 0, 1, 0x01, 0xBB, 0x22, 0xB8];
        assert_eq!(
            parse_addr(0x1, &p4).unwrap(),
            Some("1.2.3.4:443".parse().unwrap())
        );
        let mut p6 = vec![0u8; 36];
        p6[15] = 1; // ::1
        p6[32] = 0x22; p6[33] = 0xB8; // port 8888
        assert_eq!(
            parse_addr(0x2, &p6).unwrap(),
            Some("[::1]:8888".parse().unwrap())
        );
        assert_eq!(parse_addr(0x1, &[1, 2, 3]).unwrap(), None);
    }
}
