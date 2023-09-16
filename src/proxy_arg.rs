#[repr(C)]
#[derive(Debug)]
pub struct ProxyArg {
    pub ip_v4_addr: [u8; 4],
    pub port: u16,
}