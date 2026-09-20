//! Does a static-musl binary manage to dlopen the system's GL libraries?
//!
//! wgpu's GL backend loads libEGL/libGLESv2 at runtime via dlopen. On this box
//! those libs are glibc-linked (Mesa from Debian), so a static-musl binary ends
//! up with two libcs in one process. This isolates whether that is what breaks,
//! instead of guessing from wgpu's "gl drivers could not be loaded".
fn main() {
    unsafe {
        for name in ["libEGL.so.1", "libGLESv2.so.2", "libwayland-client.so.0"] {
            let c = std::ffi::CString::new(name).unwrap();
            let h = libc::dlopen(c.as_ptr(), libc::RTLD_NOW);
            if h.is_null() {
                let e = libc::dlerror();
                let msg = if e.is_null() {
                    "no error string".to_string()
                } else {
                    std::ffi::CStr::from_ptr(e).to_string_lossy().to_string()
                };
                println!("{name:26} FAIL: {msg}");
            } else {
                // try to resolve a symbol, which is where libc mixing usually bites
                let sym = std::ffi::CString::new("eglGetDisplay").unwrap();
                let s = libc::dlsym(h, sym.as_ptr());
                println!(
                    "{name:26} opened, eglGetDisplay {}",
                    if s.is_null() { "MISSING" } else { "resolved" }
                );
            }
        }
    }
}
