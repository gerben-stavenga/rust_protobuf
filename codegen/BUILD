proto_library(
    name = "example",
    srcs = ["example.proto"],
)

cc_proto_library(
    name = "example_cc",
    deps = [":example"],
)

cc_binary(
    name = "compiler",
    srcs = ["codegen.cpp"],
    deps = [
        "@protobuf//:protobuf",
        ":example_cc"
    ],
)
