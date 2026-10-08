//! A peer that connects after bind but before listen gets nothing from the
//! guest until it listens and accepts. Motor's native bind already listens,
//! so there the peer's connection and data queue; elsewhere it is refused.
use test_programs::sockets::MOTOR;
use test_programs::wasi::sockets::network::{
    ErrorCode, IpAddress, IpAddressFamily, IpSocketAddress, Network,
};
use test_programs::wasi::sockets::tcp::TcpSocket;

fn main() {
    let net = Network::default();
    let server = TcpSocket::new(IpAddressFamily::Ipv4).unwrap();
    let local = IpSocketAddress::new(IpAddress::IPV4_LOOPBACK, 0);
    server.blocking_bind(&net, local).unwrap();
    let addr = server.local_address().unwrap();
    assert_eq!(server.accept().err(), Some(ErrorCode::InvalidState));

    let client = TcpSocket::new(IpAddressFamily::Ipv4).unwrap();
    let connected = client.blocking_connect(&net, addr);
    if !MOTOR {
        assert_eq!(connected.err(), Some(ErrorCode::ConnectionRefused));
        return;
    }
    let (_client_input, client_output) = connected.unwrap();
    client_output.blocking_write_util(b"early").unwrap();
    assert_eq!(server.accept().err(), Some(ErrorCode::InvalidState));

    server.blocking_listen().unwrap();
    let (_accepted, input, _output) = server.blocking_accept().unwrap();
    let mut received = Vec::new();
    while received.len() < 5 {
        received.extend(input.blocking_read(5).unwrap());
    }
    assert_eq!(received, b"early");
}
