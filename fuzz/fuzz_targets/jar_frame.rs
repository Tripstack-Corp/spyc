//! The jar's frame parser, on both sides of its socket: one frame body, then
//! the bytes read as a stream of frames. Neither may panic or loop.
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let [kind, flags, payload @ ..] = data {
        let _ = spyc_jar::proto::decode(*kind, *flags, payload);
    }
    let mut stream = data;
    while !stream.is_empty() {
        let before = stream.len();
        if spyc_jar::proto::read_msg(&mut stream).is_err() || stream.len() == before {
            break;
        }
    }
});
