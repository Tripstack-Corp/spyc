//! Generate spyc-vt-sys's FFI bindings from the PIN's headers.
//!
//! Run once per pin bump; the output is checked in, so neither bindgen nor
//! libclang is a dependency of the shipped crate. Kept out of the crate itself
//! for that reason.
fn main() {
    let inc = std::env::args().nth(1).expect("usage: bindgen-tool <ghostty/include> <out.rs>");
    let out = std::env::args().nth(2).expect("out path");
    let hdr = format!("{inc}/ghostty/vt.h");
    let b = bindgen::Builder::default()
        .header(&hdr)
        .clang_arg(format!("-I{inc}"))
        // Only ghostty's own surface; no libc spill.
        .allowlist_function("ghostty_.*")
        .allowlist_type("Ghostty.*")
        .allowlist_var("GHOSTTY_.*")
        // Enums as plain integer constants: the C headers pin them to `int`
        // (GHOSTTY_ENUM_TYPED : int) and give explicit values, so a Rust enum
        // would add a validity invariant the C side does not promise.
        .default_enum_style(bindgen::EnumVariation::ModuleConsts)
        .derive_debug(true)
        .derive_copy(true)
        .derive_default(false)
        .layout_tests(true)
        .generate_comments(true)
        // NOT `use_core()`. bindgen's layout assertions are emitted as
        // compile-time `const _: () = { ["Size of X"][size_of::<X>() - N]; }`
        // blocks that name `::std::mem`, so `use_core()` would not compile them.
        // Those assertions are the guard that catches ABI drift on a pin bump —
        // the exact failure this crate exists to contain — so they stay, and the
        // crate is std anyway.
        .generate()
        .expect("bindgen");
    b.write_to_file(&out).expect("write");
    let text = std::fs::read_to_string(&out).expect("read back");
    println!("wrote {out}: {} lines", text.lines().count());
}
