//! Version 1 of the jar's wire, frozen: one frame of every message kind,
//! and the handover blob, byte for byte. A jar released with version 1 speaks
//! these bytes for as long as it runs, so a change here is a new version, not
//! an edit (`docs/drafts/JAR_PROTOCOL.md` → "Version 1 on the wire").
use spyc_jar::proto::{self, Msg, Role, SNAPSHOT_VT, Snapshot};

const FRAMES: &[(&str, &str)] = &[
    (
        "hello (owner)",
        "000000170180000101001030313233343536373839616263646566",
    ),
    ("hello (lookup)", "0000000701800001030000"),
    ("input", "0000000502806c730d"),
    ("resize", "0000000a03800028007800000000"),
    ("ack", "0000000a04800000000000001000"),
    ("replay", "0000000a05800000000000000000"),
    ("signal", "00000006068000000001"),
    (
        "store_snapshot",
        "00000012078000020000000000000006000200036869",
    ),
    ("fetch_snapshot", "000000020880"),
    ("ping", "0000000a09800000000000000007"),
    ("ack_exit", "000000020a80"),
    ("reply", "000000060b801b5b306e"),
    ("upgrade", "000000080c802f62696e2f6a"),
    ("describe", "000000020d80"),
    ("release", "000000120e8066656463626139383736353433323130"),
    ("set_info", "0000000c0f8000000002697370616e65"),
    (
        "welcome",
        "0000001c40800001000010e1000000000000007d0000000000000064001e0064",
    ),
    ("output", "0000000c418000000000000000646869"),
    ("resized", "0000000e42800000000000000066001e0064"),
    ("gap", "00000012438000000000000000000000000000000064"),
    ("snapshot (none)", "00000003448000"),
    (
        "snapshot (some)",
        "0000001344800100020000000000000006000200036869",
    ),
    ("pong", "0000000a45800000000000000007"),
    ("exited", "0000000746800000000f01"),
    ("refused", "0000000447806e6f"),
    ("unsupported", "00000003488063"),
    ("foreground", "00000006498000001092"),
    ("upgraded", "000000044a006232"),
    ("description", "000000104b800000000269730000000470616e65"),
];

const HANDOVER: &str = concat!(
    "535059434a41524800010000001a2f746d702f737079632f6a6172732f73616d",
    "706c652e736f636b00000003000000072f62696e2f7368000000022d63000000",
    "076563686f20686901000000042f746d70002800780000000000400000000000",
    "00001000000000000000002710000000000000ea60000100000000000493e012",
    "3456789abcdef0000000006aee4480000010e100000000000000070000000000",
    "00007d0000000000000064000000040000000000000003000000000000000064",
    "0000000668656c6c6f20000000000000000401000000000000006a001e006400",
    "0000000000000500000000000000006a00000007776f726c640d0a0000000000",
    "0000060000000000000000710000000c1b5b316d626f6c641b5b306d01000000",
    "0000000002000000000000006e000000020001000000000000006a001e006400",
    "0000030102030002000000000000006a001e0064000000091b5b4868656c6c6f",
    "200100000000000001001e006400000000000000040000001092010000001030",
    "31323334353637383961626364656600000001000000096f776e65725f6d6370",
    "000000122f746d702f737079632f6d63702e736f636b00000002000000000000",
    "000201010000000000000007000000000000007d000000109200000000000000",
    "0302000000000000",
);

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn messages() -> Vec<(&'static str, Msg)> {
    let snap = Snapshot {
        format: SNAPSHOT_VT,
        offset: 6,
        rows: 2,
        cols: 3,
        blob: b"hi".to_vec(),
    };
    vec![
        (
            "hello (owner)",
            Msg::Hello {
                version: 1,
                role: Role::Owner,
                token: b"0123456789abcdef".to_vec(),
            },
        ),
        (
            "hello (lookup)",
            Msg::Hello {
                version: 1,
                role: Role::Lookup,
                token: vec![],
            },
        ),
        (
            "input",
            Msg::Input {
                bytes: b"ls\r".to_vec(),
            },
        ),
        (
            "resize",
            Msg::Resize {
                rows: 40,
                cols: 120,
                px_w: 0,
                px_h: 0,
            },
        ),
        ("ack", Msg::Ack { offset: 4096 }),
        ("replay", Msg::Replay { from: 0 }),
        ("signal", Msg::Signal { signal: 1 }),
        ("store_snapshot", Msg::StoreSnapshot(snap.clone())),
        ("fetch_snapshot", Msg::FetchSnapshot),
        ("ping", Msg::Ping { n: 7 }),
        ("ack_exit", Msg::AckExit),
        (
            "reply",
            Msg::Reply {
                bytes: b"\x1b[0n".to_vec(),
            },
        ),
        (
            "upgrade",
            Msg::Upgrade {
                binary: "/bin/j".into(),
            },
        ),
        ("describe", Msg::Describe),
        (
            "release",
            Msg::Release {
                token: b"fedcba9876543210".to_vec(),
            },
        ),
        (
            "set_info",
            Msg::SetInfo {
                key: "is".into(),
                value: "pane".into(),
            },
        ),
        (
            "welcome",
            Msg::Welcome {
                version: 1,
                child_pid: 4321,
                head: 125,
                base: 100,
                rows: 30,
                cols: 100,
            },
        ),
        (
            "output",
            Msg::Output {
                offset: 100,
                bytes: b"hi".to_vec(),
            },
        ),
        (
            "resized",
            Msg::Resized {
                offset: 102,
                rows: 30,
                cols: 100,
            },
        ),
        ("gap", Msg::Gap { from: 0, to: 100 }),
        ("snapshot (none)", Msg::Snapshot(None)),
        ("snapshot (some)", Msg::Snapshot(Some(snap))),
        ("pong", Msg::Pong { n: 7 }),
        (
            "exited",
            Msg::Exited {
                code: 15,
                signaled: true,
            },
        ),
        ("refused", Msg::Refused { why: "no".into() }),
        ("unsupported", Msg::Unsupported { kind: 99 }),
        ("foreground", Msg::Foreground { pgid: 4242 }),
        ("upgraded", Msg::Upgraded { build: "b2".into() }),
        (
            "description",
            Msg::Description {
                fields: vec![("is".into(), "pane".into())],
            },
        ),
    ]
}

#[test]
fn every_message_kind_encodes_to_its_frozen_bytes_and_back() {
    let msgs = messages();
    assert_eq!(msgs.len(), FRAMES.len());
    for ((name, m), (fname, hex)) in msgs.iter().zip(FRAMES) {
        assert_eq!(name, fname);
        let frame = unhex(hex);
        assert_eq!(proto::encode(m), frame, "{name} encodes to its fixture");
        let back = proto::read_msg(&mut frame.as_slice()).expect("reads");
        assert_eq!(back.as_ref(), Some(m), "{name} decodes from its fixture");
    }
}

#[test]
fn the_handover_blob_is_byte_for_byte_version_1() {
    let blob = unhex(HANDOVER);
    assert_eq!(spyc_jar::handover::sample_blob(), blob);
    spyc_jar::handover::validate(&blob).expect("version 1's blob validates");
}

#[test]
fn a_hello_with_capabilities_after_its_token_still_reads() {
    let mut frame = unhex(FRAMES[0].1);
    frame.extend(b"\x00\x01caps");
    let len = u32::try_from(frame.len() - 4).expect("small");
    frame[..4].copy_from_slice(&len.to_be_bytes());
    let back = proto::read_msg(&mut frame.as_slice()).expect("reads");
    assert_eq!(back, Some(messages().remove(0).1));
}
