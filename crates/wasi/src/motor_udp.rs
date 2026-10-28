use super::*;

/// One WASI socket owns one native endpoint; wildcard selection is native.
pub struct UdpSocket {
    family: SocketAddressFamily,
    socket: Mutex<Option<Arc<tokio::net::UdpSocket>>>,
    remote: Mutex<Option<SocketAddr>>,
    options: Mutex<BTreeMap<u64, u64>>,
}
impl UdpSocket {
    pub fn new(family: SocketAddressFamily) -> Self {
        Self {
            family,
            socket: Mutex::new(None),
            remote: Mutex::new(None),
            options: Mutex::new(BTreeMap::new()),
        }
    }
    fn unspecified(&self) -> SocketAddr {
        crate::sockets::unspecified_addr(self.family)
    }
    fn bind_locked(
        &self,
        slot: &mut Option<Arc<tokio::net::UdpSocket>>,
        addr: SocketAddr,
    ) -> stdio::Result<()> {
        if slot.is_some() {
            return Err(stdio::ErrorKind::InvalidInput.into());
        }
        let socket = std::net::UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        for (&opt, &val) in self.options.lock().unwrap().iter() {
            if opt != moto_rt::net::SO_ONLY_IPV6 {
                native_set(socket.as_raw_fd(), opt, val).map_err(stdio::Error::from)?;
            }
        }
        *slot = Some(Arc::new(tokio::net::UdpSocket::from_std(socket)?));
        Ok(())
    }
    pub(super) fn bind(&self, addr: SocketAddr) -> stdio::Result<()> {
        self.bind_locked(&mut self.socket.lock().unwrap(), addr)
    }
    fn ensure_bound(&self) -> stdio::Result<Arc<tokio::net::UdpSocket>> {
        let mut slot = self.socket.lock().unwrap();
        if slot.is_none() {
            self.bind_locked(&mut slot, self.unspecified())?;
        }
        Ok(slot.as_ref().unwrap().clone())
    }
    pub(super) fn connect(&self, remote: SocketAddr) -> Result<(), io::Errno> {
        self.ensure_bound()?;
        *self.remote.lock().unwrap() = Some(remote);
        Ok(())
    }
    pub(super) fn disconnect(&self) {
        *self.remote.lock().unwrap() = None;
    }
    pub fn local_addr(&self) -> stdio::Result<SocketAddr> {
        match self.socket.lock().unwrap().as_ref() {
            Some(socket) => socket.local_addr(),
            None => Ok(self.unspecified()),
        }
    }
    pub fn peer_addr(&self) -> stdio::Result<SocketAddr> {
        self.remote
            .lock()
            .unwrap()
            .ok_or(stdio::ErrorKind::NotConnected.into())
    }
    pub async fn writable(&self) -> stdio::Result<()> {
        // An unbound socket is writable; binding here would make every new
        // socket bound before the guest's own bind. Sending binds it lazily.
        let socket = self.socket.lock().unwrap().clone();
        match socket {
            Some(socket) => socket.writable().await,
            None => Ok(()),
        }
    }
    pub async fn send(&self, b: &[u8]) -> stdio::Result<usize> {
        self.send_to(b, self.peer_addr()?).await
    }
    pub async fn send_to(&self, b: &[u8], addr: SocketAddr) -> stdio::Result<usize> {
        self.ensure_bound()?.send_to(b, addr).await
    }
    pub async fn recv_from(&self, b: &mut [u8]) -> stdio::Result<(usize, SocketAddr)> {
        let socket = self.ensure_bound()?;
        loop {
            let result = socket.recv_from(b).await?;
            let peer = *self.remote.lock().unwrap();
            if peer.is_none() || peer == Some(result.1) {
                return Ok(result);
            }
        }
    }
    pub(super) fn option(&self, opt: u64) -> Result<u64, io::Errno> {
        if opt == moto_rt::net::SO_ONLY_IPV6 {
            return Ok(u64::from(matches!(self.family, SocketAddressFamily::Ipv6)));
        }
        if opt != moto_rt::net::SO_TTL {
            return Err(io::Errno::OPNOTSUPP);
        }
        if let Some(socket) = self.socket.lock().unwrap().as_ref() {
            return native_get(socket.as_raw_fd(), opt);
        }
        Ok(*self.options.lock().unwrap().get(&opt).unwrap_or(&64))
    }
    pub(super) fn set_option(&self, opt: u64, val: u64) -> Result<(), io::Errno> {
        if opt == moto_rt::net::SO_ONLY_IPV6 {
            return if val == u64::from(matches!(self.family, SocketAddressFamily::Ipv6)) {
                Ok(())
            } else {
                Err(io::Errno::OPNOTSUPP)
            };
        }
        if opt != moto_rt::net::SO_TTL {
            return Err(io::Errno::OPNOTSUPP);
        }
        if let Some(socket) = self.socket.lock().unwrap().as_ref() {
            native_set(socket.as_raw_fd(), opt, val)?;
        }
        self.options.lock().unwrap().insert(opt, val);
        Ok(())
    }
}
