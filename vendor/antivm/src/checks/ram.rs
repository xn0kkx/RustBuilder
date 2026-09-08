use sysinfo::System;

use crate::config::MIN_RAM_BYTES;
use crate::protection::Protection;

impl Protection {
    pub(crate) fn check_ram(&self) {
        let mut sys = System::new();
        sys.refresh_memory();
        if sys.total_memory() < MIN_RAM_BYTES {
            self.on_fail();
        }
    }
}
