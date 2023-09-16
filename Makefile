cbindgen_api:
	cbindgen --config cbindgen.toml --crate network_spy_proxy --lang c --output include/api.h

build_lib_release:
	~/.cargo/bin/cargo build --release --target aarch64-apple-darwin   

build_lib_debug:
	~/.cargo/bin/cargo build --target aarch64-apple-darwin   

gen_lib:
	make cbindgen_api
	make build_lib_release
	