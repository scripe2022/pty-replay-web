use anyhow::{Context, anyhow, bail};
use base64::Engine as _;
use flate2::read::GzDecoder;
use serde::Deserialize;
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use std::io::Read;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", content = "data")]
enum Event {
    Heartbeat(Vec<(OffsetDateTime, usize)>),
    Cast(String, String),
    #[allow(dead_code)]
    Info(OffsetDateTime, String),
    #[allow(dead_code)]
    Warning(OffsetDateTime, String),
    #[allow(dead_code)]
    Error(OffsetDateTime, String),
}

impl TryFrom<&str> for Event {
    type Error = anyhow::Error;

    fn try_from(line: &str) -> anyhow::Result<Self> {
        let (at, ev) = line.split_once(' ').context("Invalid log line")?;
        let at = OffsetDateTime::parse(at, &Rfc3339).context("Invalid timestamp")?;

        let (kind, payload): (String, Value) = serde_json::from_str(ev).context("Invalid event")?;

        match kind.as_str() {
            "info" | "warning" | "error" => {
                let payload = payload
                    .as_str()
                    .ok_or_else(|| anyhow!("{kind} expects string payload"))?
                    .to_owned();
                match kind.as_str() {
                    "info" => Ok(Event::Info(at, payload)),
                    "warning" => Ok(Event::Warning(at, payload)),
                    "error" => Ok(Event::Error(at, payload)),
                    _ => unreachable!(),
                }
            }
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

pub fn parse_log(buf: &str) -> (Vec<HBRaw>, Vec<CastRaw>) {
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
