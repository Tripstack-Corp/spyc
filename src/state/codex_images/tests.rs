use super::*;
use serde_json::json;

const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
const PARENT: &str = "019e8b21-9e7c-7553-a118-d1cdada725fd";
const FORK: &str = "019e8b21-9e7c-7553-a118-d1cdada725fe";
const LEAF: &str = "019e8b21-9e7c-7553-a118-d1cdada725ff";

fn inline(data: &str) -> serde_json::Value {
    json!({"type":"image", "image_url":format!("data:image/png;base64,{data}")})
}
fn item(id: &str, text: &str, images: &[serde_json::Value]) -> String {
    let mut content = vec![json!({"type":"text", "text":text})];
    content.extend_from_slice(images);
    json!({"type":"event_msg", "payload":{"type":"item_completed", "item":{"type":"UserMessage", "id":id, "content":content}}}).to_string() + "\n"
}
fn api(text: &str, images: &[&str]) -> String {
    let mut content = vec![json!({"type":"input_text", "text":text})];
    content.extend(images.iter().map(
        |data| json!({"type":"input_image", "image_url":format!("data:image/png;base64,{data}")}),
    ));
    json!({"type":"response_item","payload":{"type":"message","role":"user","content":content}})
        .to_string()
        + "\n"
}
fn meta(id: &str, cwd: &Path, parent: Option<(&str, usize)>) -> String {
    let mut payload = json!({"id":id,"cwd":cwd});
    if let Some((thread, end)) = parent {
        payload["history_base"] = json!({"thread_id":thread,"end_byte_offset":end});
    }
    json!({"type":"session_meta","payload":payload}).to_string() + "\n"
}
fn write(root: &Path, id: &str, body: &str) -> PathBuf {
    let day = root.join("sessions/2026/10/12");
    std::fs::create_dir_all(&day).unwrap();
    let path = day.join(format!("rollout-now-{id}.jsonl"));
    std::fs::write(&path, body).unwrap();
    path
}
fn bytes() -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(PNG)
        .unwrap()
}
fn loaded(path: &Path, image: &TranscriptImage) -> Vec<u8> {
    super::super::transcript_images::load(path, image).unwrap()
}

#[test]
fn current_turn_items_and_api_twins_show_once_but_repeat_submissions_remain() {
    let tmp = tempfile::tempdir().unwrap();
    let submitted = item(
        "one",
        "compare [Image #1] [Image #2]",
        &[inline(PNG), inline("Yg==")],
    );
    let path = write(
        tmp.path(),
        PARENT,
        &(submitted.clone()
            + &submitted
            + &api("compare", &[PNG, "Yg=="])
            + &item("two", "compare", &[inline(PNG), inline("Yg==")])),
    );
    let images = index(&path);
    assert_eq!(
        images.len(),
        4,
        "mirror and replay are duplicates; a later submission is not"
    );
    assert_eq!(images[0].agent_label.as_deref(), Some("#1"));
    assert_eq!(images[1].agent_label.as_deref(), Some("#2"));
    assert_eq!(loaded(&path, &images[0]), bytes());
    assert_eq!(loaded(&path, &images[1]), b"b");
    assert_eq!(images[3].seq, 4);
}

#[test]
fn legacy_local_paths_use_the_recorded_cwd_and_inline_twins_survive_deleted_files() {
    let tmp = tempfile::tempdir().unwrap();
    let image = tmp.path().join("attachment.png");
    std::fs::write(&image, bytes()).unwrap();
    let event=json!({"type":"event_msg","payload":{"type":"user_message","message":"look [Image #1]","local_images":["attachment.png"]}}).to_string()+"\n";
    let path = write(
        tmp.path(),
        PARENT,
        &(meta(PARENT, tmp.path(), None) + &event + &api_local("look", "attachment.png")),
    );
    let images = index(&path);
    assert_eq!(images.len(), 1);
    assert_eq!(loaded(&path, &images[0]), bytes());
    std::fs::remove_file(&image).unwrap();
    assert_eq!(
        loaded(&path, &images[0]),
        bytes(),
        "durable API bytes remain after Codex's temporary file disappears"
    );
}

#[test]
fn legacy_api_only_rollouts_and_event_urls_are_supported() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(tmp.path(), PARENT, &api("old history", &[PNG]));
    assert_eq!(loaded(&path, &index(&path)[0]), bytes());
    let event=json!({"type":"event_msg","payload":{"type":"user_message","message":"remote input","images":[format!("data:image/png;base64,{PNG}")]}}).to_string()+"\n";
    let path = write(tmp.path(), FORK, &event);
    assert_eq!(loaded(&path, &index(&path)[0]), bytes());
}

#[test]
fn fork_of_a_fork_loads_parent_bytes_only_before_each_branch_point() {
    let tmp = tempfile::tempdir().unwrap();
    let parent = meta(PARENT, tmp.path(), None) + &item("root", "inherited", &[inline(PNG)]);
    write(
        tmp.path(),
        PARENT,
        &(parent.clone() + &item("later", "parent only", &[inline("Yg==")])),
    );
    let mid = meta(FORK, tmp.path(), Some((PARENT, parent.len())))
        + &item("mid", "middle", &[inline("Yw==")]);
    write(
        tmp.path(),
        FORK,
        &(mid.clone() + &item("later", "middle only", &[inline("ZA==")])),
    );
    let leaf = write(
        tmp.path(),
        LEAF,
        &(meta(LEAF, tmp.path(), Some((FORK, mid.len())))
            + &item("leaf", "leaf", &[inline("ZQ==")])),
    );
    let images = index(&leaf);
    assert_eq!(
        images
            .iter()
            .map(|i| i.prompt_excerpt.as_str())
            .collect::<Vec<_>>(),
        ["inherited", "middle", "leaf"]
    );
    assert_eq!(loaded(&leaf, &images[0]), bytes());
    assert_eq!(loaded(&leaf, &images[1]), b"c");
    assert_eq!(loaded(&leaf, &images[2]), b"e");
    assert_eq!(
        index(&leaf),
        images,
        "reopening a resumed gallery is stable"
    );
}

#[test]
fn fork_ancestry_stays_in_its_home_and_cycles_stop() {
    let tmp = tempfile::tempdir().unwrap();
    let first = tmp.path().join("first");
    let second = tmp.path().join("second");
    let parent = meta(PARENT, tmp.path(), None) + &item("parent", "correct", &[inline(PNG)]);
    write(&first, PARENT, &parent);
    write(
        &second,
        PARENT,
        &item("wrong", "wrong home", &[inline("Yg==")]),
    );
    let path = write(
        &first,
        FORK,
        &meta(FORK, tmp.path(), Some((PARENT, parent.len()))),
    );
    let images = index(&path);
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].prompt_excerpt, "correct");
    assert_eq!(loaded(&path, &images[0]), bytes());
    write(
        &first,
        PARENT,
        &meta(PARENT, tmp.path(), Some((FORK, usize::MAX))),
    );
    assert!(index(&path).is_empty(), "cyclic metadata does not hang");
}

#[test]
fn missing_oversized_nonregular_and_malformed_images_fail_without_allocating_them() {
    let tmp = tempfile::tempdir().unwrap();
    let large = tmp.path().join("large.png");
    std::fs::File::create(&large)
        .unwrap()
        .set_len(MAX_IMAGE_BYTES + 1)
        .unwrap();
    let path = write(
        tmp.path(),
        PARENT,
        &item(
            "one",
            "unavailable",
            &[
                json!({"type":"local_image","path":tmp.path().join("missing.png")}),
                json!({"type":"local_image","path":large}),
                json!({"type":"local_image","path":tmp.path()}),
                inline("="),
                inline(&"é".repeat(80)),
                json!({"type":"image","image_url":"https://example.invalid/image.png"}),
                json!({"type":"image","file_id":"file-opaque"}),
            ],
        ),
    );
    let images = index(&path);
    assert_eq!(images.len(), 7);
    for image in images {
        assert!(super::super::transcript_images::load(&path, &image).is_err());
    }
}

#[test]
fn oversized_records_do_not_hide_following_images_and_indexes_reject_rewrites() {
    let tmp = tempfile::tempdir().unwrap();
    let body = "x".repeat(super::super::MAX_TRANSCRIPT_LINE_BYTES + 1)
        + "\n"
        + &item("one", "after oversized", &[inline(PNG)]);
    let path = write(tmp.path(), PARENT, &body);
    let images = index(&path);
    assert_eq!(images.len(), 1);
    assert_eq!(loaded(&path, &images[0]), bytes());
    let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    use std::io::Write;
    let ImageSource::Codex { primary, .. } = &images[0].source else {
        panic!("Codex source")
    };
    file.seek(SeekFrom::Start(primary.offset)).unwrap();
    let replacement = item(
        "one",
        "after oversized",
        &[inline(&PNG.replacen("iVBOR", "aVBOR", 1))],
    );
    file.write_all(replacement.as_bytes()).unwrap();
    let error = super::super::transcript_images::load(&path, &images[0]).unwrap_err();
    assert!(error.contains("transcript changed"), "{error}");
}

#[test]
fn assistant_and_tool_images_are_not_user_attachments() {
    let tmp = tempfile::tempdir().unwrap();
    let path=write(tmp.path(),PARENT,&[
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[inline(PNG)]}}).to_string(),
        json!({"type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","content":[inline(PNG)]}}}).to_string(),
    ].join("\n"));
    assert!(index(&path).is_empty());
}

#[test]
fn index_caps_rows_and_scan_bytes_even_for_many_small_records() {
    let tmp = tempfile::tempdir().unwrap();
    let first = item("one", "first", &[inline(PNG)]);
    let path = write(
        tmp.path(),
        PARENT,
        &(first.clone() + &item("two", "second", &[inline(PNG)])),
    );
    let mut rows = Vec::new();
    let mut budget = first.len() as u64;
    index_file(&path, u64::MAX, &mut budget, &mut rows);
    assert_eq!(rows.len(), 1);
    assert_eq!(budget, 0);
    let many = (0..=MAX_IMAGES)
        .map(|i| item(&i.to_string(), "same image", &[inline("Yg==")]))
        .collect::<String>();
    let path = write(tmp.path(), PARENT, &many);
    assert_eq!(index(&path).len(), MAX_IMAGES);
}

#[test]
fn an_api_local_image_tag_does_not_create_a_duplicate_row() {
    let tmp = tempfile::tempdir().unwrap();
    let current = item(
        "one",
        "look [Image #1]",
        &[json!({"type":"local_image","path":"/tmp/a.png"})],
    );
    let response=json!({"type":"response_item","payload":{"type":"message","role":"user","content":[
        {"type":"input_text","text":"look"},{"type":"input_text","text":"<image name=[Image #1] path=\"/tmp/a.png\">"},
        {"type":"input_image","image_url":format!("data:image/png;base64,{PNG}")},{"type":"input_text","text":"</image>"},
    ]}}).to_string()+"\n";
    let path = write(tmp.path(), PARENT, &(response + &current));
    assert_eq!(
        index(&path).len(),
        1,
        "API-first and current-first mirrors both coalesce"
    );
}

fn api_local(text: &str, path: &str) -> String {
    json!({"type":"response_item","payload":{"type":"message","role":"user","content":[
        {"type":"input_text","text":text},
        {"type":"input_text","text":format!("<image name=[Image #1] path=\"{path}\">")},
        {"type":"input_image","image_url":format!("data:image/png;base64,{PNG}")},
        {"type":"input_text","text":"</image>"},
    ]}})
    .to_string()
        + "\n"
}

#[test]
fn equal_prompts_do_not_merge_different_attachments_or_cross_turn_completion() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        PARENT,
        &(item("one", "same prompt", &[inline(PNG)]) + &api("same prompt", &["Yg=="])),
    );
    assert_eq!(index(&path).len(), 2, "prompt text is not image identity");
    let end = json!({"type":"event_msg","payload":{"type":"turn_complete"}}).to_string() + "\n";
    let path = write(
        tmp.path(),
        FORK,
        &(item("one", "same prompt", &[inline(PNG)]) + &end + &api("same prompt", &[PNG])),
    );
    assert_eq!(
        index(&path).len(),
        2,
        "a later turn is a separate attachment"
    );
}

#[test]
fn malformed_legacy_slots_do_not_shift_a_later_images_label() {
    let tmp = tempfile::tempdir().unwrap();
    let event=json!({"type":"event_msg","payload":{"type":"user_message","message":"[Image #1] [Image #2]", "images":[42,format!("data:image/png;base64,{PNG}")],"file_ids":["opaque"]}}).to_string()+"\n";
    let path = write(tmp.path(), PARENT, &event);
    let images = index(&path);
    assert_eq!(images.len(), 3);
    assert_eq!(images[1].agent_label.as_deref(), Some("#2"));
    assert_eq!(loaded(&path, &images[1]), bytes());
    assert!(super::super::transcript_images::load(&path, &images[0]).is_err());
    assert!(super::super::transcript_images::load(&path, &images[2]).is_err());
}

#[test]
fn legacy_image_order_keeps_opaque_file_slots_before_inline_attachments() {
    let tmp = tempfile::tempdir().unwrap();
    let event=json!({"type":"event_msg","payload":{"type":"user_message","message":"[Image #1] [Image #2]", "images":[format!("data:image/png;base64,{PNG}")],"file_ids":["opaque"],"image_order":["file","inline"]}}).to_string()+"\n";
    let path = write(tmp.path(), PARENT, &event);
    let images = index(&path);
    assert_eq!(images.len(), 2);
    assert!(super::super::transcript_images::load(&path, &images[0]).is_err());
    assert_eq!(images[1].agent_label.as_deref(), Some("#2"));
    assert_eq!(loaded(&path, &images[1]), bytes());
}
