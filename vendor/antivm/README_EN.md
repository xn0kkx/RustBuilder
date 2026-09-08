# 🛡️ anti-vm

A Rust library for protecting your program from running in an "unwanted" environment.

---

## Features

| Check | What it does |
|---|---|
| 🌍 **IP Filter** | Terminates if the public IP belongs to an EU or NATO country |
| 🔗 **HTTP Filter** | Terminates if the specified URL returned `200 OK` |
| 💻 **VM Filter** | Terminates when VirtualBox or VMware drivers are detected |
| 📡 **Network** | Terminates if there is no internet access |
| 🖥️ **Screen** | Terminates if the screen resolution is 800×600 |
| ⚙️ **CPU** | Terminates if only 1 logical core is available |
| 🧠 **RAM** | Terminates if RAM is less than 4 GB |

---

## Installation

Add the dependency to your project's `Cargo.toml`:

```toml
[dependencies]
anti-vm = "0.1"
```

---

## Quick Start

```rust
use anti_vm::ProtectionBuilder;

fn main() {
    // Run all checks
    ProtectionBuilder::new()
        .set_vm(true)
        .set_network(true)
        .set_screen(true)
        .set_cpu(true)
        .set_ram(true)
        .set_ip(true)
        .set_http(true)
        .init(); // <-- if a check fails, the program will silently exit here

    // Code will only reach here if everything is fine
    println!("Welcome!");
    // ... rest of your program
}
```

---

## Configuring Filters

Each filter can be **enabled** (`true`) or **disabled** (`false`) independently of the others.

### `.set_vm(bool)` — Virtual Machine Check

Searches for VirtualBox and VMware driver files in Windows system directories.

```rust
ProtectionBuilder::new()
    .set_vm(true)  // enabled by default
    .init();
```

**What is checked:**

| VirtualBox | VMware |
|---|---|
| `VBoxGuest.sys` | `vmxnet3.sys` |
| `VBoxVideo.sys` | `vm3d.sys` |
| `VBoxWddm.sys` | `vmwvxpe.sys` |
| `VBoxSF.sys` | `vmmemctl.sys` |
| `VBoxMouse.sys` | `vmci.sys` |
| `VBoxService.exe` | `vmhgfs.sys` |
| | `vmvss.sys` |
| | `pvscsi.sys` |
| | `vmblock.sys` |

**Search directories:**
- `C:\Windows\System32\drivers`
- `C:\Windows\System32`
- `C:\Windows\SysWOW64\drivers`
- `C:\Windows\SysWOW64`

---

### `.set_network(bool)` — Internet Connectivity Check

Attempts to establish a TCP connection to `8.8.8.8:53` (Google DNS). If it fails — terminates the process.

```rust
ProtectionBuilder::new()
    .set_network(true)  // enabled by default
    .init();
```
---

### `.set_screen(bool)` — Screen Resolution Check

Reads the primary monitor's resolution via the Windows API (`GetSystemMetrics`). If width = 800 and height = 600 — terminates. This resolution is typical of virtual machines without guest additions installed.

```rust
ProtectionBuilder::new()
    .set_screen(true)  // enabled by default
    .init();
```

---

### `.set_cpu(bool)` — CPU Core Count Check

Determines the number of logical processor cores. If ≤ 1 — terminates. Virtual machines are often created with only 1 core.

```rust
ProtectionBuilder::new()
    .set_cpu(true)  // enabled by default
    .init();
```

---

### `.set_ram(bool)` — RAM Amount Check

Reads the total amount of RAM. If less than **4 GB** — terminates.

```rust
ProtectionBuilder::new()
    .set_ram(true)  // enabled by default
    .init();
```

---

### `.set_ip(bool)` — IP Address Geolocation Check

```rust
ProtectionBuilder::new()
    .set_ip(true)  // enabled by default
    .init();
```

**Allowed countries (CIS):**
`RU` `BY` `UA` `KZ` `TJ` `UZ` `KG` `AM` `AZ` `GE` `MD` `TM`

**Blocked countries:**

<details>
<summary>Show full list (EU + NATO)</summary>

**EU:** AT BE BG HR CY CZ DK EE FI FR DE GR HU IE IT LV LT LU MT NL PL PT RO SK SI ES SE

**NATO (not in EU):** AL CA IS ME MK NO TR GB US

</details>

---

### `.set_http(bool)` — HTTP Check via External URL

Performs a GET request to the specified URL. If the server returns `200 OK` — terminates. Used as an additional "kill switch": if your server returns 200, the program will shut down.

```rust
ProtectionBuilder::new()
    .set_http(true)  // enabled by default
    .init();
```

---

## Additional Parameters

### Change the URL for the HTTP check

```rust
ProtectionBuilder::new()
    .set_http(true)
    .http_url("http://my-kill-switch.example.com")
    .init();
```

### Change the geolocation service URL

```rust
ProtectionBuilder::new()
    .set_ip(true)
    .ip_api_url("http://ip-api.com/json/?fields=status,countryCode")
    .init();
```

### Change the network request timeout

```rust
ProtectionBuilder::new()
    .timeout(3)  // 3 seconds
    .init();
```

---

### `.filtered(|| ...)` — What to do when a check fails

By default, if any filter fails, the program silently terminates via `exit(0)`. If you pass a function to `.filtered(...)`, that function will be called instead of exiting the program when a check fails.

```rust
use anti_vm::ProtectionBuilder;

fn main() {
    ProtectionBuilder::new()
        .set_vm(true)
        .set_http(true)
        .filtered(|| {
            // Your own logic instead of terminating the program
            println!("Looks like an unwanted environment!");
        })
        .init();

    // Code continues to run, even if a filter fails
    println!("Program continues to run");
}
```

> **Note:** if `.filtered(...)` is not set, the behavior remains the same — the program exits when any filter fails.

---

## Check Execution Order

```
1. VM drivers     — local, instant
2. Screen         — local, instant
3. CPU            — local, instant
4. RAM            — local, instant
──────────────────────────────────────
5. Network        — requires internet
6. IP geolocation — requires internet
7. HTTP request   — requires internet
```
