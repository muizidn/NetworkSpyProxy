use std::{os::raw::c_char, ffi::CString};
use std::fmt::Write;
use bytes::Bytes;
use hudsucker::HttpContext;
use hyper::{Body, Request, Response};


pub struct ReqBody {
    pub ctx: HttpContext,
    pub val: Request<Bytes>
}

impl ReqBody  {
    pub fn new(ctx: HttpContext, val: Request<Bytes>) -> ReqBody {
        ReqBody { ctx, val }
    }
}

pub struct ResBody {
    pub ctx:HttpContext,
    pub val:Response<Bytes>
}

impl ResBody  {
    pub fn new(ctx: HttpContext, val: Response<Bytes>) -> ResBody {
        ResBody { ctx, val }
    }
}

#[no_mangle]
pub extern "C" fn req_body_free(ptr: *mut ReqBody) {
    assert!(!ptr.is_null());
    unsafe {
        Box::from_raw(ptr);
    }
}

#[no_mangle]
pub extern "C" fn res_body_free(ptr: *mut ResBody) {
    assert!(!ptr.is_null());
    unsafe {
        Box::from_raw(ptr);
    }
}

// REQUEST

#[no_mangle]
pub extern "C" fn req_body_http_context_ip(ptr: *mut ReqBody) -> *mut c_char {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    let ip = r.ctx.client_addr.ip().to_string().clone();
    CString::new(ip).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn req_body_http_context_ip_free(ptr: *mut c_char) {
    unsafe {
        assert!(!ptr.is_null());
        CString::from_raw(ptr)
    };
}

#[no_mangle]
pub extern "C" fn req_body_http_context_port(ptr: *mut ReqBody) -> u16 {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    let port = r.ctx.client_addr.port();
    port
}

#[no_mangle]
pub extern "C" fn req_body_http_uri(ptr: *mut ReqBody) -> *mut c_char {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    let val = r.val.uri().to_string().clone();
    CString::new(val).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn req_body_http_uri_free(ptr: *mut c_char) {
    unsafe {
        assert!(!ptr.is_null());
        CString::from_raw(ptr)
    };
}

#[no_mangle]
pub extern "C" fn req_body_http_method(ptr: *mut ReqBody) -> *mut c_char {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    let val = r.val.method().to_string().clone();
    CString::new(val).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn req_body_http_method_free(ptr: *mut c_char) {
    unsafe {
        assert!(!ptr.is_null());
        CString::from_raw(ptr)
    };
}

#[no_mangle]
pub extern "C" fn req_body_http_headers(ptr: *mut ReqBody) -> *mut c_char {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    let mut header_formated = String::new();
    for (key, value) in r.val.headers() {
        let v = match value.to_str() {
            Ok(v) => v.to_string(),
            Err(_) => {
                format!("[u8]; {}", value.len())
            }
        };
        write!(
            &mut header_formated,
            "\t{:<20}{}\r\n",
            format!("{}:", key.as_str()),
            v
        )
        .unwrap();
    }
    CString::new(header_formated).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn req_body_http_headers_free(ptr: *mut c_char) {
    unsafe {
        assert!(!ptr.is_null());
        CString::from_raw(ptr)
    };
}

#[no_mangle]
pub extern "C" fn req_body_http_version(ptr: *mut ReqBody) -> *mut c_char {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    let val = format!("{:?}", r.val.version());
    CString::new(val).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn req_body_http_version_free(ptr: *mut c_char) {
    unsafe {
        assert!(!ptr.is_null());
        CString::from_raw(ptr)
    };
}

#[no_mangle]
pub extern "C" fn req_body_http_body_len(ptr: *mut ReqBody) -> usize {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    r.val.body().len()
}

#[no_mangle]
pub extern "C" fn req_body_http_write_body(ptr: *mut ReqBody, data: *mut u8) {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    let body = r.val.body().to_vec();
    unsafe { std::ptr::copy(body.as_ptr(), data, body.len()); }
}

// RESPONSE

#[no_mangle]
pub extern "C" fn res_body_http_context_ip(ptr: *mut ResBody) -> *mut c_char {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    let ip = r.ctx.client_addr.ip().to_string().clone();
    CString::new(ip).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn res_body_http_context_ip_free(ptr: *mut c_char) {
    unsafe {
        assert!(!ptr.is_null());
        CString::from_raw(ptr)
    };
}

#[no_mangle]
pub extern "C" fn res_body_http_context_port(ptr: *mut ResBody) -> u16 {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    let port = r.ctx.client_addr.port();
    port
}

#[no_mangle]
pub extern "C" fn res_body_http_status(ptr: *mut ResBody) -> u16 {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    let val = r.val.status().as_u16();
    val
}

#[no_mangle]
pub extern "C" fn res_body_http_version(ptr: *mut ResBody) -> *mut c_char {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    let val = format!("{:?}", r.val.version());
    CString::new(val).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn res_body_http_version_free(ptr: *mut c_char) {
    unsafe {
        assert!(!ptr.is_null());
        CString::from_raw(ptr)
    };
}

#[no_mangle]
pub extern "C" fn res_body_http_headers(ptr: *mut ResBody) -> *mut c_char {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    let mut header_formated = String::new();
    for (key, value) in r.val.headers() {
        let v = match value.to_str() {
            Ok(v) => v.to_string(),
            Err(_) => {
                format!("[u8]; {}", value.len())
            }
        };
        write!(
            &mut header_formated,
            "\t{:<20}{}\r\n",
            format!("{}:", key.as_str()),
            v
        )
        .unwrap();
    }
    CString::new(header_formated).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn res_body_http_headers_free(ptr: *mut c_char) {
    unsafe {
        assert!(!ptr.is_null());
        CString::from_raw(ptr)
    };
}

#[no_mangle]
pub extern "C" fn res_body_http_body_len(ptr: *mut ResBody) -> usize {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    r.val.body().len()
}

#[no_mangle]
pub extern "C" fn res_body_http_write_body(ptr: *mut ResBody, data: *mut u8) {
    let r = unsafe {
        assert!(!ptr.is_null());
        &*ptr
    };
    let body = r.val.body().to_vec();
    unsafe { std::ptr::copy(body.as_ptr(), data, body.len()); }
}