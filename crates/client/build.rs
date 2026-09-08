use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=OMC_BUILD_ID");
    println!("cargo:rerun-if-env-changed=OMC_SERVER_URL");
    println!("cargo:rerun-if-env-changed=OMC_CA_CERT");
    println!("cargo:rerun-if-env-changed=OMC_CLIENT_IDENTITY");
    println!("cargo:rerun-if-env-changed=OMC_BUILD_PUBKEY");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    let build_id = env::var("OMC_BUILD_ID").unwrap_or_else(|_| "dev-build".to_string());
    let server_url =
        env::var("OMC_SERVER_URL").unwrap_or_else(|_| "https://127.0.0.1:8443".to_string());

    let ca_path = resolve_asset(&out_dir, "ca.pem", env::var("OMC_CA_CERT").ok());
    let identity_path =
        resolve_asset(&out_dir, "identity.pem", env::var("OMC_CLIENT_IDENTITY").ok());
    let pubkey_path = resolve_asset(&out_dir, "pubkey.asc", env::var("OMC_BUILD_PUBKEY").ok());

    let config = format!(
        "pub const BUILD_ID: &str = {build_id:?};\n\
         pub const SERVER_URL: &str = {server_url:?};\n\
         pub const CA_CERT: &[u8] = include_bytes!({ca:?});\n\
         pub const CLIENT_IDENTITY: &[u8] = include_bytes!({identity:?});\n\
         pub const PUB_KEY: &[u8] = include_bytes!({pubkey:?});\n",
        ca = ca_path,
        identity = identity_path,
        pubkey = pubkey_path,
    );

    fs::write(out_dir.join("config.rs"), config).unwrap();
}

fn resolve_asset(out_dir: &std::path::Path, name: &str, src: Option<String>) -> String {
    match src {
        Some(path) if !path.is_empty() => path,
        _ => {
            let fallback = out_dir.join(name);
            if !fallback.exists() {
                fs::write(&fallback, b"").unwrap();
            }
            fallback.to_string_lossy().to_string()
        }
    }
}
