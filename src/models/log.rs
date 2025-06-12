use anyhow::{Context, bail};
use base64::Engine as _;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::sync::LazyLock;
use time::OffsetDateTime;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", content = "data")]
enum Event {
    Heartbeat(Vec<OffsetDateTime>),
    Cast(u128, Vec<u8>),
}

impl TryFrom<&str> for Event {
    type Error = anyhow::Error;

    fn try_from(line: &str) -> anyhow::Result<Self> {
        let (kind, payload): (String, Value) = serde_json::from_str(line).context("Invalid event")?;

        match kind.as_str() {
            "cast" => {
                let (filename, content): (u128, String) =
                    serde_json::from_value(payload).context("cast payload expects [filename, content]")?;

                let compressed = base64::engine::general_purpose::STANDARD.decode(&content)?;
                let cast = zstd::stream::decode_all(&compressed[..])
                    .context("Failed to decompress cast payload with zstd")?;
                Ok(Event::Cast(filename, cast))
            }
            "heartbeat" => {
                let content: String =
                    serde_json::from_value(payload).context("heratbeat payload expects [timestamps]")?;
                let compressed = base64::engine::general_purpose::STANDARD.decode(&content)?;
                let raw = zstd::stream::decode_all(&compressed[..])
                    .context("Failed to decompress heartbeat payload with zstd")?;
                let beats = raw.chunks_exact(4).map(|chunk| {
                    let ts = u32::from_le_bytes(chunk.try_into()?) as i64;
                    OffsetDateTime::from_unix_timestamp(ts).context("Invalid heartbeat timestamp")
                }).collect::<anyhow::Result<Vec<OffsetDateTime>>>()?;
                Ok(Event::Heartbeat(beats))
            }
            _ => bail!("Unknown event type {kind}"),
        }
    }
}

#[derive(Debug)]
pub struct CastRaw {
    pub filename: String,
    pub content: Vec<u8>,
}

static TS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z ").unwrap());

pub fn strip_timestamps<S: AsRef<str>>(input: S) -> String {
    TS_RE.replace_all(input.as_ref(), "").into_owned()
}

pub fn parse_log(buf: &str) -> (Vec<OffsetDateTime>, Vec<CastRaw>) {
    let buf = strip_timestamps(buf);
    let lines = buf.lines().collect::<Vec<_>>();
    let events = lines
        .into_iter()
        .filter_map(|line| Event::try_from(line).ok())
        .collect::<Vec<_>>();

    let hbs_raw = events
        .iter()
        .filter_map(|x| match x {
            Event::Heartbeat(times) => Some(times.iter().cloned()),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();

    let mut casts_map = std::collections::HashMap::<u128, Vec<u8>>::new();
    events
        .into_iter()
        .filter_map(|x| match x {
            Event::Cast(filename, content) => Some((filename, content)),
            _ => None,
        })
        .for_each(|(filename, content)| {
            casts_map.entry(filename).or_default().extend(content);
        });

    let casts_raw = casts_map
        .into_iter()
        .map(|(filename, content)| CastRaw { filename: format!("{filename}"), content })
        .collect::<Vec<_>>();

    (hbs_raw, casts_raw)
}
