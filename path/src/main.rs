// src/main.rs
//! Main entry point.

use crate::pager_handler::mod;

fn main() {
    let mut pager_handler = PagerHandler::create();
    let buffer = "Hello, World!";
    let output = pager_handler.render(buffer);
    println!("{}", output);
}