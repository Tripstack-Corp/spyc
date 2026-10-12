//! Readable image inputs in Codex's current turn items and legacy rollout records.
use serde_json::Value;

pub const CURRENT: u8 = 1;
pub const LEGACY: u8 = 2;
pub const API: u8 = 4;

#[derive(Clone, Copy)]
pub enum Input<'a> {
    Inline { url: &'a str, path: Option<&'a str> },
    Local(&'a str),
    Unavailable,
}

pub struct Record<'a> {
    pub kind: u8,
    pub id: Option<&'a str>,
    pub text: String,
    pub images: Vec<Input<'a>>,
}

pub fn decode(value: &Value) -> Option<Record<'_>> {
    let payload = &value["payload"];
    match (value["type"].as_str(), payload["type"].as_str()) {
        (Some("event_msg"), Some("item_completed")) => {
            let item = &payload["item"];
            (item["type"] == "UserMessage").then(|| from_content(item, CURRENT))
        }
        (Some("event_msg"), Some("user_message")) => {
            let images = legacy_images(payload);
            Some(Record {
                kind: LEGACY,
                id: payload["client_id"].as_str(),
                text: payload["message"].as_str().unwrap_or_default().to_owned(),
                images,
            })
        }
        (Some("response_item"), Some("message")) if payload["role"] == "user" => {
            Some(from_content(payload, API))
        }
        _ => None,
    }
}

fn from_content(value: &Value, kind: u8) -> Record<'_> {
    let mut text = Vec::new();
    let mut images = Vec::new();
    let mut image_path = None;
    if let Some(content) = value["content"].as_array() {
        for block in content {
            match block["type"].as_str() {
                Some("text" | "input_text") => {
                    if let Some(part) = block["text"].as_str() {
                        // API messages wrap local images in separate tag blocks.
                        if kind == API && part.starts_with("<image ") {
                            image_path = part
                                .split_once(" path=\"")
                                .and_then(|(_, path)| path.strip_suffix("\">"));
                        }
                        if !(kind == API && (part.starts_with("<image ") || part == "</image>")) {
                            text.push(part);
                        }
                    }
                }
                Some("image" | "input_image") => {
                    images.push(
                        block["image_url"]
                            .as_str()
                            .map_or(Input::Unavailable, |url| Input::Inline {
                                url,
                                path: image_path.take(),
                            }),
                    );
                }
                Some("local_image") => {
                    images.push(
                        block["path"]
                            .as_str()
                            .map_or(Input::Unavailable, Input::Local),
                    );
                }
                _ => {}
            }
        }
    }
    Record {
        kind,
        id: value["client_id"].as_str().or_else(|| value["id"].as_str()),
        text: text.join(" "),
        images,
    }
}

pub fn data_url(url: &str) -> Option<(&str, &str)> {
    let (header, data) = url.strip_prefix("data:")?.split_once(',')?;
    let media = header.strip_suffix(";base64")?;
    (media.starts_with("image/") && media.len() <= 128).then_some((media, data))
}

fn legacy_images(payload: &Value) -> Vec<Input<'_>> {
    let mut inline = payload["images"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|url| {
            url.as_str()
                .map_or(Input::Unavailable, |url| Input::Inline { url, path: None })
        });
    let mut files = payload["file_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|_| Input::Unavailable);
    let mut images = Vec::new();
    if let Some(order) = payload["image_order"].as_array() {
        for kind in order {
            images.push(match kind.as_str() {
                Some("inline") => inline.next().unwrap_or(Input::Unavailable),
                Some("file") => files.next().unwrap_or(Input::Unavailable),
                _ => Input::Unavailable,
            });
        }
    }
    images.extend(inline);
    images.extend(files);
    images.extend(
        payload["local_images"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|path| path.as_str().map_or(Input::Unavailable, Input::Local)),
    );
    images
}
