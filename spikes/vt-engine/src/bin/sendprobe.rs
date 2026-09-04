//! Does each engine's terminal type satisfy the bounds spyc's pane already
//! requires? `Pane` holds `parser: Arc<Mutex<vt100::Parser>>`, hands a clone to
//! a dedicated parser worker thread, and locks it from the render pass — so the
//! type must be `Send`. This compiles only for the engines that are.
fn assert_send<T: Send>() {}

fn main() {
    assert_send::<vt100::Parser>();
    println!("vt100::Parser        : Send  (Arc<Mutex<_>> across the worker thread is fine)");

    #[cfg(feature = "wezterm")]
    {
        assert_send::<wezterm_term::Terminal>();
        println!("wezterm_term::Terminal: Send");
    }

    #[cfg(feature = "ghostty")]
    {
        // Uncommenting this is the experiment; it must FAIL to compile.
        // assert_send::<libghostty_vt::Terminal<'static, 'static>>();
        println!("libghostty_vt::Terminal: see the compile-fail check below");
    }
}
