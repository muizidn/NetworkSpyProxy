use std::{os::raw::c_char};

extern {
    fn get_ip_addr() -> *mut c_char;
    fn free_ip_addr(ip: *mut c_char);
}

#[no_mangle]
pub extern "C" fn get_ip_address() -> *mut c_char {
    unsafe {
        get_ip_addr()
    }
}

#[no_mangle]
pub extern "C" fn get_ip_address_free(ptr: *mut c_char) {
    unsafe {
        assert!(!ptr.is_null());
        // free_ip_addr(ptr); // causes crash
    };
}