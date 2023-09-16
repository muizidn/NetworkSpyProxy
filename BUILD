package(default_visibility = ["//visibility:public"])
load("@build_bazel_rules_swift//swift:swift.bzl", "swift_c_module", "swift_library", "swift_test")

genrule(
    name = "create_staticlib_with_cargo",
    srcs = glob([
        "src/*",
        "include/api.h"
    ]) + [
        ":Cargo.toml",
        ":cbindgen.toml",
        ":Makefile"
    ],
    outs = [
        "libnetwork_spy_proxy.a"
    ],
    cmd = """
    config="debug"
    # Because the build will run from root project
    # then we need to change to this directory
    cd NetworkSpyProxy
    make build_lib_$${config}
    cd ..
    mv NetworkSpyProxy/target/aarch64-apple-darwin/$${config}/libnetwork_spy_proxy.a $(OUTS)
    """
)

cc_import(
    name = "cc",
    hdrs = [
        "include/api.h",
    ],
    static_library = ":create_staticlib_with_cargo"
)

swift_c_module(
  name = "NetworkSpyProxyRust",
  deps = [":cc"],
  module_map = "include/module.modulemap",
  module_name = "NetworkSpyProxyRust"
)

swift_library(
    name = "NetworkSpyProxy",
    module_name = "NetworkSpyProxy",
    srcs = glob([
         "swift/Source/*.swift",
    ]),
    deps = [
        ":NetworkSpyProxyRust",
        "//:OpenSSL",
        "//:Crypto"
    ]
)

swift_test(
    name = "NetworkSpyProxyTest",
    srcs = glob([
         "swift/Test/*.swift",
    ]),
    deps = [
        ":NetworkSpyProxy",
    ],
    features = [],
)