# codegen/protocrap.bzl
load("@rules_proto//proto:defs.bzl", "ProtoInfo")

def _protocrap_aspect_impl(target, ctx):
    """Generate Rust code for proto_library targets."""
    
    # Check if this target has ProtoInfo
    if ProtoInfo not in target:
        return []
    
    proto_info = target[ProtoInfo]
    
    outputs = []
    for src in proto_info.direct_sources:
        # Generate output filename based on .proto filename
        out = ctx.actions.declare_file(
            src.basename.replace(".proto", ".pc.rs"),
        )
        outputs.append(out)
        
        # Build proto_path arguments
        args = ctx.actions.args()
        args.add("--plugin=protoc-gen-protocrap=" + ctx.executable._protocrap_plugin.path)
        args.add("--protocrap_out=" + out.dirname)
        
        # Add proto_path for the source directory
        args.add("--proto_path=" + src.dirname)
        
        # Add proto_path for all transitive imports
        for proto_path in proto_info.transitive_proto_path.to_list():
            args.add("--proto_path=" + proto_path)
        
        # Add the source file
        args.add(src.path)
        
        ctx.actions.run(
            inputs = depset(
                direct = proto_info.direct_sources,
                transitive = [proto_info.transitive_sources],
            ),
            outputs = [out],
            executable = ctx.executable._protoc,
            arguments = [args],
            tools = [ctx.executable._protocrap_plugin],
            mnemonic = "ProtocrapGen",
            progress_message = "Generating Rust code for %s" % src.short_path,
        )
    
    return [
        DefaultInfo(files = depset(outputs)),
    ]

protocrap_aspect = aspect(
    implementation = _protocrap_aspect_impl,
    attrs = {
        "_protoc": attr.label(
            default = "@protobuf//:protoc",
            executable = True,
            cfg = "exec",
        ),
        "_protocrap_plugin": attr.label(
            default = "//codegen:protoc-gen-protocrap",
            executable = True,
            cfg = "exec",
        ),
    },
    attr_aspects = ["deps"],
)

def _protocrap_library_impl(ctx):
    """Collect generated files from all deps."""
    
    files = []
    for dep in ctx.attr.deps:
        if DefaultInfo in dep:
            files.append(dep[DefaultInfo].files)
    
    return [
        DefaultInfo(files = depset(transitive = files)),
    ]

protocrap_library = rule(
    implementation = _protocrap_library_impl,
    attrs = {
        "deps": attr.label_list(
            aspects = [protocrap_aspect],
            providers = [ProtoInfo],
            mandatory = True,
        ),
    },
)