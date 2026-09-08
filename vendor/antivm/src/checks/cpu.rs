use crate::protection::Protection;

impl Protection {
    pub(crate) fn check_cpu(&self) {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        if cores <= 1 {
            self.on_fail();
        }
    }
}
