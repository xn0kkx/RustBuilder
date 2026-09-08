fn main() {
    #[cfg(windows)]
    {
        if std::env::var("CARGO_FEATURE_SHOW_NOTIFICATION").is_ok() {
            use winapi::um::winuser::{MessageBoxW, MB_OK};
            unsafe {
                let title: Vec<u16> = "msg at developer\0".encode_utf16().collect();
                let text: Vec<u16> = "Thanks for using lib 'antivm'\nСпасибо за использование библиотеки 'antivm'\n\n\nmy github: github.com/northernboykisser\nmy telegram: @nrthbkser\n\n\nIf you don't want to see this, modify Cargo.toml.\nЕсли не хотите видеть это измените cargo.toml\n:\nantivm = { version = ver, default-features = false }\0".encode_utf16().collect();
                MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK);
            }
        }
    }
}
