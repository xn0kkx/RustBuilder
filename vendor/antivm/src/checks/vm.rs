use std::path::Path;

use crate::config::{VM_DRIVERS, VM_DRIVER_DIRS};
use crate::protection::Protection;

impl Protection {
    pub(crate) fn check_vm(&self) {
        for dir in VM_DRIVER_DIRS {
            for driver in VM_DRIVERS {
                if Path::new(dir).join(driver).exists() {
                    self.on_fail();
                }
            }
        }
    }
}
