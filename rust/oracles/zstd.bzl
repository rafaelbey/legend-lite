"""zstd_decompress: a committed .zst oracle as the plain file a test reads, by @bazel_lib's hermetic zstd."""

def _zstd_decompress_impl(ctx):
    zstd = ctx.toolchains["@bazel_lib//lib:zstd_toolchain_type"].zstdinfo.binary
    out = ctx.actions.declare_file(ctx.attr.out)
    ctx.actions.run(
        executable = zstd,
        arguments = ["-q", "-d", "-f", ctx.file.src.path, "-o", out.path],
        inputs = [ctx.file.src],
        outputs = [out],
        mnemonic = "ZstdDecompress",
        progress_message = "Decompressing %{input}",
    )
    return [DefaultInfo(files = depset([out]), runfiles = ctx.runfiles(files = [out]))]

zstd_decompress = rule(
    implementation = _zstd_decompress_impl,
    doc = "The plain text of a committed .zst file, for a test that reads it (rust/oracles: the frozen JVM answers).",
    attrs = {
        "src": attr.label(allow_single_file = [".zst"], mandatory = True),
        "out": attr.string(mandatory = True, doc = "the decompressed file's name"),
    },
    toolchains = ["@bazel_lib//lib:zstd_toolchain_type"],
)
