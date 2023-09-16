# CMakeLists.txt

# Define a custom target to generate API headers using cbindgen
add_custom_target(cbindgen_api
    COMMAND cbindgen --config cbindgen.toml --crate network_spy_proxy --lang c --output include/api.h
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    COMMENT "Generating API headers using cbindgen"
)

# Build the Rust library in release mode
add_custom_target(build_lib_release
    COMMAND cargo build --release --target aarch64-apple-darwin
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    COMMENT "Building Rust library in release mode"
)

# Build the Rust library in debug mode
add_custom_target(build_lib_debug
    COMMAND cargo build --target aarch64-apple-darwin
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    COMMENT "Building Rust library in debug mode"
)

# Combine cbindgen and Rust library build targets into a single target
add_custom_target(gen_lib
    DEPENDS cbindgen_api build_lib_release
)

# Build OpenSSL (assuming it's in the 'openssl' directory)
add_custom_target(build_openssl
    COMMAND cd openssl && ./Configure && make
    WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
    COMMENT "Building OpenSSL"
)

# Add a custom target to build everything at once
add_custom_target(build_all
    DEPENDS gen_lib build_openssl
)
