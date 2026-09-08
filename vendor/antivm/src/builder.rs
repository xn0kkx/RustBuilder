use crate::protection::Protection;

pub struct ProtectionBuilder {
    inner: Protection,
}

impl ProtectionBuilder {
    pub fn new() -> Self {
        Self { inner: Protection::default() }
    }

    pub fn set_ip(mut self, enabled: bool) -> Self {
        self.inner.ip_filter = enabled;
        self
    }

    pub fn set_http(mut self, enabled: bool) -> Self {
        self.inner.http_filter = enabled;
        self
    }

    pub fn set_vm(mut self, enabled: bool) -> Self {
        self.inner.vm_filter = enabled;
        self
    }

    pub fn set_network(mut self, enabled: bool) -> Self {
        self.inner.network_filter = enabled;
        self
    }

    pub fn set_screen(mut self, enabled: bool) -> Self {
        self.inner.screen_filter = enabled;
        self
    }

    pub fn set_cpu(mut self, enabled: bool) -> Self {
        self.inner.cpu_filter = enabled;
        self
    }

    pub fn set_ram(mut self, enabled: bool) -> Self {
        self.inner.ram_filter = enabled;
        self
    }

    pub fn http_url(mut self, url: impl Into<String>) -> Self {
        self.inner.http_url = url.into();
        self
    }

    pub fn ip_api_url(mut self, url: impl Into<String>) -> Self {
        self.inner.ip_api_url = url.into();
        self
    }

    pub fn timeout(mut self, secs: u64) -> Self {
        self.inner.timeout_secs = secs;
        self
    }

    pub fn filtered(mut self, callback: impl Fn() + 'static) -> Self {
        self.inner.filter_callback = Some(Box::new(callback));
        self
    }

    pub fn init(self) {
        self.inner.run();
    }
}

impl Default for ProtectionBuilder {
    fn default() -> Self {
        Self::new()
    }
}
