extern crate cc;

fn main() {
    cc::Build::new()
        .file("src/c/ip_addr.c")
        .compile("libipaddress.a");
}