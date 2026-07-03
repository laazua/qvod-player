//! Windows resource compilation (icon, version info, manifest).
//!
//! On Windows targets this embeds `icon.rc` into the executable, setting the
//! application icon and version metadata.  On non-Windows targets it is a
//! no-op.

fn main() {
    // Only compile the resource on Windows targets; the call is a no-op
    // elsewhere.
    // `embed_resource::compile` is a no-op on non-Windows targets.
    // Rerun if either resource file changes.
    println!("cargo:rerun-if-changed=assets/icon.rc");
    println!("cargo:rerun-if-changed=assets/qvod.ico");

    embed_resource::compile("assets/icon.rc");
}
