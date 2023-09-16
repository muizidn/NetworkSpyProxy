use std::net::Ipv4Addr;
use std::net::SocketAddrV4;

use tokio::runtime::{Runtime, Builder};
use tracing::error;

use crate::openssl::*;
use crate::traffic::*;
use crate::proxy_arg::*;

pub struct Proxy {
    arg: ProxyArg,
    runtime: Runtime,
    openssl: Option<OpenSSL>
}

impl Proxy {

    fn new(arg: ProxyArg) -> Proxy {
        let runtime = Builder::new_multi_thread()
        .worker_threads(4)
        .thread_name("network-spy-proxy")
        .thread_stack_size(3 * 1024 * 1024)
        .enable_io()
        .enable_time()
        .build()
        .unwrap();
        return Proxy { arg, runtime, openssl: None }
    }
}

#[no_mangle]
pub extern "C" fn proxy_new(arg:ProxyArg) -> *mut Proxy {
    Box::into_raw(Box::new(Proxy::new(arg)))
}

#[no_mangle]
pub extern  "C" fn proxy_listen(
    ptr: *mut Proxy, 
    req_callback: *mut fn(u8, *mut ReqBody),
    res_callback: *mut fn(u8, *mut ResBody),
    id: u8
) 
    {
    let proxy = unsafe {
        assert!(!ptr.is_null());
        &mut *ptr
    };
    let req_callback = unsafe { Box::from_raw(req_callback) };
    let res_callback = unsafe { Box::from_raw(res_callback) };
    let runtime = &proxy.runtime;
    let ip = Ipv4Addr::new(
        proxy.arg.ip_v4_addr[0],
        proxy.arg.ip_v4_addr[1],
        proxy.arg.ip_v4_addr[2],
        proxy.arg.ip_v4_addr[3]);
    let port = proxy.arg.port;
    let openssl = OpenSSL { addr: SocketAddrV4::new(ip, port) };
    proxy.openssl = Some(openssl);
    let borrow = &proxy.openssl;
    runtime.block_on(async {
        let handler = TrafficHandler {
            fn_req: |id, callback,ctx, ptr| {
                let req = Box::into_raw(Box::new(ReqBody::new(ctx, ptr)));
                callback(id, req);
            },
            fn_res: |id, callback,ctx, ptr| {
                let res = Box::into_raw(Box::new(ResBody::new(ctx, ptr)));
                callback(id, res);
            },
            req_callback: req_callback,
            res_callback: res_callback,
            id: id
        };
        Option::expect(borrow.as_ref(), "").run(handler).await
    });
}

#[no_mangle]
pub extern "C" fn proxy_unlisten(ptr: *mut Proxy, id: u8) {
    let http = unsafe {
        assert!(!ptr.is_null());
        &mut *ptr
    };
    error!("Unimplemented");
}

#[no_mangle]
pub extern "C" fn proxy_free(ptr: *mut Proxy) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        Box::from_raw(ptr);
    }
}