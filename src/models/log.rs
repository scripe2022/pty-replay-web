use anyhow::{Context, anyhow, bail};
use base64::Engine as _;
use flate2::read::GzDecoder;
use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::sync::LazyLock;
use time::OffsetDateTime;

use std::io::Read;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", content = "data")]
enum Event {
    Heartbeat(Vec<(OffsetDateTime, usize)>),
    Cast(String, String),
}

impl TryFrom<&str> for Event {
    type Error = anyhow::Error;

    fn try_from(line: &str) -> anyhow::Result<Self> {
        let (kind, payload): (String, Value) = serde_json::from_str(line).context("Invalid event")?;

        match kind.as_str() {
            "cast" => {
                let (filename, content): (String, String) =
                    serde_json::from_value(payload).context("cast payload expects [filename, content]")?;

                let compressed = base64::engine::general_purpose::STANDARD.decode(&content)?;
                let mut cast = String::new();
                GzDecoder::new(compressed.as_slice())
                    .read_to_string(&mut cast)
                    .context("Failed to decompress cast payload")?;
                Ok(Event::Cast(filename, cast))
            }
            "heartbeat" => {
                let raw: Vec<(i64, usize)> =
                    serde_json::from_value(payload).context("heratbeat payload expects [[timestamp, unsigned],..]")?;
                let beats = raw
                    .into_iter()
                    .map(|(ts, v)| {
                        OffsetDateTime::from_unix_timestamp(ts)
                            .map(|t| (t, v))
                            .map_err(|e| anyhow!("bad heartbeat ts {ts}: {e}"))
                    })
                    .collect::<anyhow::Result<Vec<(OffsetDateTime, usize)>>>()?;
                Ok(Event::Heartbeat(beats))
            }
            _ => bail!("Unknown event type {kind}"),
        }
    }
}

#[derive(Debug)]
pub struct HBRaw {
    pub time: OffsetDateTime,
    pub session: usize,
}

#[derive(Debug)]
pub struct CastRaw {
    pub filename: String,
    pub content: String,
}

static TS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z ").unwrap());

pub fn strip_timestamps<S: AsRef<str>>(input: S) -> String {
    TS_RE.replace_all(input.as_ref(), "").into_owned()
}

pub fn parse_log(buf: &str) -> (Vec<HBRaw>, Vec<CastRaw>) {
    let buf = strip_timestamps(buf);
    let lines = buf.lines().collect::<Vec<_>>();
    let events = lines
        .into_iter()
        .filter_map(|line| Event::try_from(line).ok())
        .collect::<Vec<_>>();

    let hbs_raw = events
        .iter()
        .filter_map(|x| match x {
            Event::Heartbeat(session) => Some(session.iter().map(|&(time, session)| HBRaw { time, session })),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();

    let mut casts_map = std::collections::HashMap::<String, String>::new();
    events
        .into_iter()
        .filter_map(|x| match x {
            Event::Cast(filename, content) => Some((filename, content)),
            _ => None,
        })
        .for_each(|(filename, content)| {
            casts_map.entry(filename).or_default().push_str(&content);
        });

    let casts_raw = casts_map
        .into_iter()
        .map(|(filename, content)| CastRaw { filename, content })
        .collect::<Vec<_>>();

    (hbs_raw, casts_raw)
}
