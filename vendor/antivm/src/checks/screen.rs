use crate::protection::Protection;

impl Protection {
    pub(crate) fn check_screen(&self) {
        #[cfg(windows)]
        {
            use winapi::um::winuser::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
            let w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
            let h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
            if w == 800 && h == 600 {
                self.on_fail();
            }
        }
    }
}
