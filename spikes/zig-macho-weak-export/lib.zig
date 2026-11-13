fn a(x: u32) callconv(.c) u32 {
    return x *% 3;
}
fn b(x: u32) callconv(.c) u32 {
    return x *% 5;
}
fn c(x: u32) callconv(.c) u32 {
    return x *% 7;
}
comptime {
    @export(&a, .{ .name = "weak_default", .linkage = .weak });
    @export(&b, .{ .name = "weak_hidden", .linkage = .weak, .visibility = .hidden });
    @export(&c, .{ .name = "strong_hidden", .visibility = .hidden });
}
