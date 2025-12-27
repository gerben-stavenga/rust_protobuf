"""Rule to generate a merged descriptor set with all transitive imports."""

def _proto_descriptor_set_impl(ctx):
    # Collect all transitive descriptor sets (depset dedupes shared deps)
    descriptor_sets = depset(transitive = [
        dep[ProtoInfo].transitive_descriptor_sets for dep in ctx.attr.deps
    ]).to_list()

    # Merge all descriptor sets into one
    output = ctx.actions.declare_file(ctx.label.name + ".bin")

    # Use cat to concatenate all descriptor sets
    # FileDescriptorSet is a repeated field, so concatenation works
    ctx.actions.run_shell(
        inputs = descriptor_sets,
        outputs = [output],
        command = "cat {} > {}".format(
            " ".join([f.path for f in descriptor_sets]),
            output.path,
        ),
    )

    return [DefaultInfo(files = depset([output]))]

proto_descriptor_set = rule(
    implementation = _proto_descriptor_set_impl,
    attrs = {
        "deps": attr.label_list(
            providers = [ProtoInfo],
            doc = "proto_library targets to include",
        ),
    },
    doc = "Generates a merged descriptor set with all transitive imports.",
)
