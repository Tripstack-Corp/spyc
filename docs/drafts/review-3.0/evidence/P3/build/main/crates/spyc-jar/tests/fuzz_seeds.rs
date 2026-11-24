//! The committed fuzz seeds are what the encoders write today. A seed is there
//! to hand the mutator a valid shape; one the decoder refuses starts every run
//! from noise, which is how the spike's seeds aged unnoticed. Each seed's
//! `write_fuzz_seed` test (ignored) writes it again.

use std::path::Path;

use spyc_jar::proto::{self, Msg};

fn seed(target: &str, name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/seeds")
        .join(target)
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn the_handover_seed_is_the_sample_blob() {
    assert_eq!(
        seed("jar_handover", "sample.bin"),
        spyc_jar::handover::sample_blob(),
        "refresh it with handover.rs's write_fuzz_seed"
    );
}

#[test]
fn the_frame_seed_reads_as_three_messages() {
    let bytes = seed("jar_frame", "hello_input_upgrade.bin");
    let mut stream = &bytes[..];
    let mut kinds = Vec::new();
    while !stream.is_empty() {
        kinds.push(
            match proto::read_msg(&mut stream).expect("the seed decodes") {
                Some(Msg::Hello { .. }) => "hello",
                Some(Msg::Input { .. }) => "input",
                Some(Msg::Upgrade { .. }) => "upgrade",
                _ => "other",
            },
        );
    }
    assert_eq!(
        kinds,
        ["hello", "input", "upgrade"],
        "refresh it with proto.rs's write_fuzz_seed"
    );
}
