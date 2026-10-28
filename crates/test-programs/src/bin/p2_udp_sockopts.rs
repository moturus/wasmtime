use test_programs::sockets::{MOTOR, udp_buffer_ok};
use test_programs::wasi::sockets::network::{ErrorCode, IpAddressFamily};
use test_programs::wasi::sockets::udp::UdpSocket;

fn test_udp_sockopt_defaults(family: IpAddressFamily) {
    let sock = UdpSocket::new(family).unwrap();

    assert_eq!(sock.address_family(), family);

    assert!(sock.unicast_hop_limit().unwrap() > 0);
    if MOTOR {
        assert!(udp_buffer_ok(sock.receive_buffer_size()));
        assert!(udp_buffer_ok(sock.send_buffer_size()));
    } else {
        assert!(sock.receive_buffer_size().unwrap() > 0);
        assert!(sock.send_buffer_size().unwrap() > 0);
    }
}

fn test_udp_sockopt_input_ranges(family: IpAddressFamily) {
    let sock = UdpSocket::new(family).unwrap();

    assert!(matches!(
        sock.set_unicast_hop_limit(0),
        Err(ErrorCode::InvalidArgument)
    ));
    assert!(matches!(sock.set_unicast_hop_limit(1), Ok(_)));
    assert!(matches!(sock.set_unicast_hop_limit(u8::MAX), Ok(_)));

    assert!(matches!(
        sock.set_receive_buffer_size(0),
        Err(ErrorCode::InvalidArgument)
    ));
    assert!(udp_buffer_ok(sock.set_receive_buffer_size(1))); // Unsupported sizes should be silently capped.
    assert!(udp_buffer_ok(sock.set_receive_buffer_size(u64::MAX))); // Unsupported sizes should be silently capped.
    assert!(matches!(
        sock.set_send_buffer_size(0),
        Err(ErrorCode::InvalidArgument)
    ));
    assert!(udp_buffer_ok(sock.set_send_buffer_size(1))); // Unsupported sizes should be silently capped.
    assert!(udp_buffer_ok(sock.set_send_buffer_size(u64::MAX))); // Unsupported sizes should be silently capped.
}

fn test_udp_sockopt_readback(family: IpAddressFamily) {
    let sock = UdpSocket::new(family).unwrap();

    sock.set_unicast_hop_limit(42).unwrap();
    assert_eq!(sock.unicast_hop_limit().unwrap(), 42);

    if MOTOR {
        assert!(udp_buffer_ok(sock.set_receive_buffer_size(0x10000)));
        assert!(udp_buffer_ok(sock.set_send_buffer_size(0x10000)));
        return;
    }

    sock.set_receive_buffer_size(0x10000).unwrap();
    assert_eq!(sock.receive_buffer_size().unwrap(), 0x10000);

    sock.set_send_buffer_size(0x10000).unwrap();
    assert_eq!(sock.send_buffer_size().unwrap(), 0x10000);
}

fn main() {
    test_udp_sockopt_defaults(IpAddressFamily::Ipv4);
    test_udp_sockopt_defaults(IpAddressFamily::Ipv6);

    test_udp_sockopt_input_ranges(IpAddressFamily::Ipv4);
    test_udp_sockopt_input_ranges(IpAddressFamily::Ipv6);

    test_udp_sockopt_readback(IpAddressFamily::Ipv4);
    test_udp_sockopt_readback(IpAddressFamily::Ipv6);
}
