"""A Rust shared library's one library file, under a name of the caller's (docs/rust-port/PHASE1.md §1).

A rust_shared_library's outputs are its library and, on Windows, the import library beside the DLL, so a rule that
takes one file (copy_file) cannot take the target. This picks the library -- .so, .dylib or .dll, the filter
tools/java_run uses -- and links it at `out`, in this package: the wheel ships the Rust compiler library beside
libcompiler (//python:wheel), where libcompiler looks for it (native/.../RustLibrary.java).
"""

def _shared_library_file_impl(ctx):
    found = [f for f in ctx.attr.src[DefaultInfo].files.to_list() if f.extension in ("so", "dylib", "dll")]
    if len(found) != 1:
        fail("%s: a shared library's target holds one library file, found %s" % (ctx.label, found))
    out = ctx.actions.declare_file(ctx.attr.out)
    ctx.actions.symlink(output = out, target_file = found[0])
    return [DefaultInfo(files = depset([out]), runfiles = ctx.runfiles(files = [out]))]

shared_library_file = rule(
    implementation = _shared_library_file_impl,
    attrs = {
        "out": attr.string(mandatory = True, doc = "The file's path in this package."),
        "src": attr.label(mandatory = True, doc = "A rust_shared_library."),
    },
    doc = "The one library file of a shared library's target, linked at `out`.",
)
