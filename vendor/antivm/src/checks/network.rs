use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use crate::config::NETWORK_CHECK_HOST;
use crate::protection::Protection;

impl Protection {
    pub(crate) fn check_network(&self) {
        let addr: SocketAddr = NETWORK_CHECK_HOST
            .parse()
            .expect("Invalid network check address");
        let timeout = Duration::from_secs(self.timeout_secs);
        if TcpStream::connect_timeout(&addr, timeout).is_err() {
            self.on_fail();
        }
    }
}
