//! Safe native socket operations for the unchanged upstream WASI state machines.
use crate::sockets::SocketAddressFamily;
use std::{
    collections::BTreeMap,
    io as stdio,
    net::SocketAddr,
    os::fd::AsRawFd,
    sync::{Arc, Mutex},
};
pub mod io {
    use super::stdio;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[allow(
        non_camel_case_types,
        reason = "matches upstream error classification names"
    )]
    pub enum Errno {
        ACCESS,
        ADDRINUSE,
        ADDRNOTAVAIL,
        AFNOSUPPORT,
        AGAIN,
        ALREADY,
        CONNABORTED,
        CONNREFUSED,
        CONNRESET,
        DESTADDRREQ,
        HOSTDOWN,
        HOSTUNREACH,
        INPROGRESS,
        INTR,
        INVAL,
        ISCONN,
        MFILE,
        MSGSIZE,
        NETDOWN,
        NETRESET,
        NETUNREACH,
        NFILE,
        NOBUFS,
        NOMEM,
        NONET,
        NOPROTOOPT,
        NOTCONN,
        OPNOTSUPP,
        PERM,
        PFNOSUPPORT,
        PIPE,
        PROTO,
        PROTONOSUPPORT,
        PROTOTYPE,
        SHUTDOWN,
        SOCKTNOSUPPORT,
        TIMEDOUT,
        WOULDBLOCK,
    }
    impl Errno {
        pub fn from_io_error(e: &stdio::Error) -> Option<Self> {
            use stdio::ErrorKind::*;
            Some(match e.kind() {
                PermissionDenied => Self::ACCESS,
                // Native sockets report an address in use as AlreadyInUse.
                AddrInUse | AlreadyExists => Self::ADDRINUSE,
                AddrNotAvailable => Self::ADDRNOTAVAIL,
                TimedOut => Self::TIMEDOUT,
                ConnectionRefused => Self::CONNREFUSED,
                ConnectionReset => Self::CONNRESET,
                ConnectionAborted => Self::CONNABORTED,
                NotConnected => Self::NOTCONN,
                WouldBlock => Self::WOULDBLOCK,
                Interrupted => Self::INTR,
                InvalidInput => Self::INVAL,
                OutOfMemory => Self::NOMEM,
                Unsupported => Self::OPNOTSUPP,
                BrokenPipe => Self::PIPE,
                NetworkUnreachable => Self::NETUNREACH,
                HostUnreachable => Self::HOSTUNREACH,
                _ => return None,
            })
        }
    }
    impl From<Errno> for stdio::Error {
        fn from(e: Errno) -> Self {
            use stdio::ErrorKind::*;
            Self::new(
                match e {
                    Errno::ACCESS | Errno::PERM => PermissionDenied,
                    Errno::ADDRINUSE => AddrInUse,
                    Errno::ADDRNOTAVAIL => AddrNotAvailable,
                    Errno::TIMEDOUT => TimedOut,
                    Errno::CONNREFUSED => ConnectionRefused,
                    Errno::CONNRESET => ConnectionReset,
                    Errno::CONNABORTED => ConnectionAborted,
                    Errno::NOTCONN => NotConnected,
                    Errno::WOULDBLOCK | Errno::AGAIN => WouldBlock,
                    Errno::INTR => Interrupted,
                    Errno::INVAL => InvalidInput,
                    Errno::NOMEM | Errno::NOBUFS => OutOfMemory,
                    Errno::OPNOTSUPP => Unsupported,
                    Errno::PIPE => BrokenPipe,
                    _ => Other,
                },
                format!("{e:?}"),
            )
        }
    }
    impl std::fmt::Display for Errno {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{self:?}")
        }
    }
    impl std::error::Error for Errno {}
    impl From<stdio::Error> for Errno {
        fn from(e: stdio::Error) -> Self {
            Self::from_io_error(&e).unwrap_or(Self::INVAL)
        }
    }
    impl From<moto_rt::Error> for Errno {
        fn from(e: moto_rt::Error) -> Self {
            Self::from(stdio::Error::from_raw_os_error(e as i32))
        }
    }
}
pub mod fd {
    use super::*;
    #[derive(Clone, Copy)]
    pub enum BorrowedFd<'a> {
        Raw(i32),
        Tcp(&'a tokio::net::TcpSocket),
        Udp(&'a UdpSocket),
    }
    pub trait AsFd {
        fn as_fd(&self) -> BorrowedFd<'_>;
    }
    impl AsFd for tokio::net::TcpSocket {
        fn as_fd(&self) -> BorrowedFd<'_> {
            BorrowedFd::Tcp(self)
        }
    }
    impl AsFd for tokio::net::TcpStream {
        fn as_fd(&self) -> BorrowedFd<'_> {
            BorrowedFd::Raw(self.as_raw_fd())
        }
    }
    impl AsFd for tokio::net::TcpListener {
        fn as_fd(&self) -> BorrowedFd<'_> {
            BorrowedFd::Raw(self.as_raw_fd())
        }
    }
    impl AsFd for UdpSocket {
        fn as_fd(&self) -> BorrowedFd<'_> {
            BorrowedFd::Udp(self)
        }
    }
    impl AsFd for BorrowedFd<'_> {
        fn as_fd(&self) -> BorrowedFd<'_> {
            *self
        }
    }
    impl<T: AsFd> AsFd for &T {
        fn as_fd(&self) -> BorrowedFd<'_> {
            (*self).as_fd()
        }
    }
    impl<T: AsFd> AsFd for Arc<T> {
        fn as_fd(&self) -> BorrowedFd<'_> {
            (**self).as_fd()
        }
    }
}
fn set(fd: impl fd::AsFd, opt: u64, val: u64) -> Result<(), io::Errno> {
    match fd.as_fd() {
        fd::BorrowedFd::Tcp(s) => s.motor_set_option(opt, val).map_err(Into::into),
        fd::BorrowedFd::Udp(s) => s.set_option(opt, val),
        fd::BorrowedFd::Raw(fd) => native_set(fd, opt, val),
    }
}
fn get(fd: impl fd::AsFd, opt: u64) -> Result<u64, io::Errno> {
    match fd.as_fd() {
        fd::BorrowedFd::Tcp(s) => s.motor_option(opt).map_err(Into::into),
        fd::BorrowedFd::Udp(s) => s.option(opt),
        fd::BorrowedFd::Raw(fd) => native_get(fd, opt),
    }
}
fn native_set(fd: i32, opt: u64, val: u64) -> Result<(), io::Errno> {
    use moto_rt::net::*;
    match opt {
        SO_TTL => set_ttl(fd, val as u32),
        SO_RCVBUF => set_recv_buffer_size(fd, val),
        SO_SNDBUF => set_send_buffer_size(fd, val),
        SO_ONLY_IPV6 => set_only_v6(fd, val != 0),
        _ => Err(moto_rt::Error::NotImplemented),
    }
    .map_err(Into::into)
}
fn native_get(fd: i32, opt: u64) -> Result<u64, io::Errno> {
    use moto_rt::net::*;
    match opt {
        SO_TTL => ttl(fd).map(u64::from),
        SO_RCVBUF => recv_buffer_size(fd),
        SO_SNDBUF => send_buffer_size(fd),
        SO_ONLY_IPV6 => only_v6(fd).map(u64::from),
        _ => Err(moto_rt::Error::NotImplemented),
    }
    .map_err(Into::into)
}
pub mod net {
    use super::*;
    pub mod sockopt {
        use super::*;
        use moto_rt::net::*;
        pub fn ip_ttl(fd: impl fd::AsFd) -> Result<u32, io::Errno> {
            get(fd, SO_TTL).map(|v| v as u32)
        }
        pub fn set_ip_ttl(fd: impl fd::AsFd, v: u32) -> Result<(), io::Errno> {
            set(fd, SO_TTL, v.into())
        }
        pub fn ipv6_unicast_hops(fd: impl fd::AsFd) -> Result<u8, io::Errno> {
            get(fd, SO_TTL).map(|v| v as u8)
        }
        pub fn set_ipv6_unicast_hops(fd: impl fd::AsFd, v: Option<u8>) -> Result<(), io::Errno> {
            set(fd, SO_TTL, v.unwrap_or(64).into())
        }
        pub fn socket_recv_buffer_size(fd: impl fd::AsFd) -> Result<usize, io::Errno> {
            get(fd, SO_RCVBUF).map(|v| v as usize)
        }
        pub fn socket_send_buffer_size(fd: impl fd::AsFd) -> Result<usize, io::Errno> {
            get(fd, SO_SNDBUF).map(|v| v as usize)
        }
        pub fn set_socket_recv_buffer_size(fd: impl fd::AsFd, v: usize) -> Result<(), io::Errno> {
            set(fd, SO_RCVBUF, v as u64)
        }
        pub fn set_socket_send_buffer_size(fd: impl fd::AsFd, v: usize) -> Result<(), io::Errno> {
            set(fd, SO_SNDBUF, v as u64)
        }
        pub fn set_ipv6_v6only(fd: impl fd::AsFd, v: bool) -> Result<(), io::Errno> {
            set(fd, SO_ONLY_IPV6, v.into())
        }
        pub fn set_socket_reuseaddr(_: impl fd::AsFd, _: bool) -> Result<(), io::Errno> {
            Err(io::Errno::OPNOTSUPP)
        }
        // Native probes run every 20 seconds with a 300-second timeout.
        // Keep synchronized with sys-io/runtime/net/socket/tcp.rs.
        pub fn socket_keepalive(_: impl fd::AsFd) -> Result<bool, io::Errno> {
            Ok(true)
        }
        pub fn set_socket_keepalive(_: impl fd::AsFd, enabled: bool) -> Result<(), io::Errno> {
            if enabled {
                Ok(())
            } else {
                Err(io::Errno::OPNOTSUPP)
            }
        }
        pub fn tcp_keepidle(_: impl fd::AsFd) -> Result<std::time::Duration, io::Errno> {
            Ok(std::time::Duration::from_secs(20))
        }
        pub fn tcp_keepintvl(_: impl fd::AsFd) -> Result<std::time::Duration, io::Errno> {
            Ok(std::time::Duration::from_secs(20))
        }
        pub fn tcp_keepcnt(_: impl fd::AsFd) -> Result<u32, io::Errno> {
            Ok(15)
        }
        pub fn set_tcp_keepidle(
            _: impl fd::AsFd,
            value: std::time::Duration,
        ) -> Result<(), io::Errno> {
            if value.is_zero() {
                Err(io::Errno::INVAL)
            } else {
                Ok(())
            }
        }
        pub fn set_tcp_keepintvl(
            fd: impl fd::AsFd,
            value: std::time::Duration,
        ) -> Result<(), io::Errno> {
            set_tcp_keepidle(fd, value)
        }
        pub fn set_tcp_keepcnt(_: impl fd::AsFd, value: u32) -> Result<(), io::Errno> {
            if value == 0 {
                Err(io::Errno::INVAL)
            } else {
                Ok(())
            }
        }
    }
    pub enum Shutdown {
        Read,
        Write,
    }
    pub fn shutdown(fd: impl fd::AsFd, how: Shutdown) -> Result<(), io::Errno> {
        let fd::BorrowedFd::Raw(fd) = fd.as_fd() else {
            return Err(io::Errno::NOTCONN);
        };
        moto_rt::net::shutdown(
            fd,
            match how {
                Shutdown::Read => moto_rt::net::SHUTDOWN_READ,
                Shutdown::Write => moto_rt::net::SHUTDOWN_WRITE,
            },
        )
        .map_err(Into::into)
    }
    pub fn listen(fd: impl fd::AsFd, backlog: i32) -> Result<(), io::Errno> {
        let fd::BorrowedFd::Raw(fd) = fd.as_fd() else {
            return Err(io::Errno::INVAL);
        };
        moto_rt::net::listen(fd, backlog as u32).map_err(Into::into)
    }
    pub fn bind(fd: impl fd::AsFd, addr: &SocketAddr) -> Result<(), io::Errno> {
        let fd::BorrowedFd::Udp(s) = fd.as_fd() else {
            return Err(io::Errno::INVAL);
        };
        s.bind(*addr).map_err(Into::into)
    }
    pub fn connect(fd: impl fd::AsFd, addr: &SocketAddr) -> Result<(), io::Errno> {
        let fd::BorrowedFd::Udp(s) = fd.as_fd() else {
            return Err(io::Errno::INVAL);
        };
        s.connect(*addr)?;
        Ok(())
    }
    pub fn connect_unspec(fd: impl fd::AsFd) -> Result<(), io::Errno> {
        let fd::BorrowedFd::Udp(s) = fd.as_fd() else {
            return Err(io::Errno::INVAL);
        };
        s.disconnect();
        Ok(())
    }
}
#[path = "motor_udp.rs"]
mod udp;
pub use udp::UdpSocket;
