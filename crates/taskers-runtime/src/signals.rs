use base64::{Engine as _, engine::general_purpose::STANDARD};
use taskers_domain::{SignalEvent, SignalKind, SignalPaneMetadata};

const OSC_PREFIX: &str = "\u{1b}]777;taskers;";
const BEL: char = '\u{7}';
const ST: &str = "\u{1b}\\";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSignal {
    pub kind: SignalKind,
    pub message: Option<String>,
    pub metadata: Option<SignalPaneMetadata>,
}

#[derive(Debug, Default, Clone)]
pub struct SignalStreamParser {
    pending: String,
}

impl ParsedSignal {
    pub fn into_event(self, source: impl Into<String>) -> SignalEvent {
        SignalEvent::with_metadata(source, self.kind, self.message, self.metadata)
    }
}

pub fn parse_signal_frames(buffer: &str) -> Vec<ParsedSignal> {
    let mut parser = SignalStreamParser::default();
    parser.push(buffer)
}

impl SignalStreamParser {
    pub fn push(&mut self, chunk: &str) -> Vec<ParsedSignal> {
        self.pending.push_str(chunk);

        let mut frames = Vec::new();
        let mut cursor = 0usize;
        let mut keep_from = floor_char_boundary(
            &self.pending,
            self.pending.len().saturating_sub(OSC_PREFIX.len()),
        );

        while let Some(found) = self.pending[cursor..].find(OSC_PREFIX) {
            let frame_start = cursor + found;
            let content_start = frame_start + OSC_PREFIX.len();
            let remainder = &self.pending[content_start..];

            let Some((raw_frame, consumed)) = frame_slice(remainder) else {
                keep_from = frame_start;
                break;
            };

            if let Some(parsed) = parse_frame(raw_frame) {
                frames.push(parsed);
            }

            cursor = content_start + consumed;
            keep_from = cursor;
        }

        self.pending = self.pending[floor_char_boundary(&self.pending, keep_from)..].to_string();
        frames
    }
}

fn parse_frame(frame: &str) -> Option<ParsedSignal> {
    let mut kind = None;
    let mut message = None;
    let mut title = None;
    let mut cwd = None;
    let mut repo_name = None;
    let mut git_branch = None;
    let mut agent_kind = None;
    let mut agent_active = None;
    let mut ports = None;

    for part in frame.split(';') {
        let (key, value) = part.split_once('=')?;
        match key {
            "kind" => {
                kind = Some(match value {
                    "metadata" => SignalKind::Metadata,
                    "started" => SignalKind::Started,
                    "progress" => SignalKind::Progress,
                    "completed" => SignalKind::Completed,
                    "waiting_input" => SignalKind::WaitingInput,
                    "error" => SignalKind::Error,
                    "notification" => SignalKind::Notification,
                    _ => return None,
                });
            }
            "message" => message = percent_decode(value),
            "message_b64" => message = decode_base64(value),
            "title" => title = percent_decode(value),
            "title_b64" => title = decode_base64(value),
            "cwd" => cwd = percent_decode(value),
            "cwd_b64" => cwd = decode_base64(value),
            "repo" | "repo_name" => repo_name = percent_decode(value),
            "repo_b64" | "repo_name_b64" => repo_name = decode_base64(value),
            "branch" | "git_branch" => git_branch = percent_decode(value),
            "branch_b64" | "git_branch_b64" => git_branch = decode_base64(value),
            "agent" | "agent_kind" => agent_kind = percent_decode(value),
            "agent_b64" | "agent_kind_b64" => agent_kind = decode_base64(value),
            "agent_active" => agent_active = parse_bool(value),
            "agent_active_b64" => {
                agent_active = decode_base64(value).and_then(|decoded| parse_bool(&decoded))
            }
            "ports" => ports = parse_ports(value),
            "ports_b64" => ports = decode_base64(value).and_then(|decoded| parse_ports(&decoded)),
            _ => {}
        }
    }

    let metadata = if title.is_some()
        || cwd.is_some()
        || repo_name.is_some()
        || git_branch.is_some()
        || agent_kind.is_some()
        || agent_active.is_some()
        || ports.is_some()
    {
        Some(SignalPaneMetadata {
            title,
            agent_title: None,
            cwd,
            repo_name,
            git_branch,
            ports: ports.unwrap_or_default(),
            agent_kind,
            agent_active,
        })
    } else {
        None
    };

    Some(ParsedSignal {
        kind: kind?,
        message,
        metadata,
    })
}

fn parse_ports(value: &str) -> Option<Vec<u16>> {
    if value.is_empty() {
        return Some(Vec::new());
    }

    value
        .split(',')
        .map(|part| part.parse::<u16>().ok())
        .collect::<Option<Vec<_>>>()
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn decode_base64(value: &str) -> Option<String> {
    let decoded = STANDARD.decode(value).ok()?;
    String::from_utf8(decoded).ok()
}

fn percent_decode(value: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(value.len());
    let raw = value.as_bytes();
    let mut index = 0usize;

    while index < raw.len() {
        match raw[index] {
            b'%' if index + 2 < raw.len() => {
                let high = decode_hex(raw[index + 1])?;
                let low = decode_hex(raw[index + 2])?;
                bytes.push((high << 4) | low);
                index += 3;
            }
            byte => {
                bytes.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8(bytes).ok()
}

fn decode_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn frame_slice(remainder: &str) -> Option<(&str, usize)> {
    if let Some(end) = remainder.find(BEL) {
        return Some((&remainder[..end], end + BEL.len_utf8()));
    }
    if let Some(end) = remainder.find(ST) {
        return Some((&remainder[..end], end + ST.len()));
    }
    None
}

fn floor_char_boundary(value: &str, mut index: usize) -> usize {
    index = index.min(value.len());
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use taskers_domain::SignalKind;

    use super::{SignalStreamParser, parse_signal_frames};

    #[test]
    fn parses_multiple_frames_with_different_terminators() {
        let output = concat!(
            "hello",
            "\u{1b}]777;taskers;kind=waiting_input;message=Need%20approval\u{7}",
            "world",
            "\u{1b}]777;taskers;kind=completed;message=Done\u{1b}\\",
        );

        let frames = parse_signal_frames(output);

        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].kind, SignalKind::WaitingInput);
        assert_eq!(frames[0].message.as_deref(), Some("Need approval"));
        assert_eq!(frames[1].kind, SignalKind::Completed);
    }

    #[test]
    fn ignores_unknown_frames() {
        let output = "\u{1b}]777;taskers;kind=unknown;message=Bad\u{7}";
        assert!(parse_signal_frames(output).is_empty());
    }

    #[test]
    fn stream_parser_handles_split_frames() {
        let mut parser = SignalStreamParser::default();

        assert!(
            parser
                .push("\u{1b}]777;taskers;kind=waiting_input;message=Need")
                .is_empty()
        );

        let frames = parser.push("%20approval\u{7}");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].kind, SignalKind::WaitingInput);
        assert_eq!(frames[0].message.as_deref(), Some("Need approval"));
    }

    #[test]
    fn stream_parser_keeps_partial_prefix_on_utf8_boundary() {
        let mut parser = SignalStreamParser::default();
        let noisy_prefix = "abbr'...\n⠙ ";
        let partial = format!("{noisy_prefix}\u{1b}]777;taskers;kind=progress;message=Working");

        assert!(parser.push(&partial).is_empty());

        let frames = parser.push("\u{7}");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].kind, SignalKind::Progress);
        assert_eq!(frames[0].message.as_deref(), Some("Working"));
    }

    #[test]
    fn parses_metadata_snapshots_with_base64_fields() {
        let output = format!(
            "\u{1b}]777;taskers;kind=metadata;cwd_b64={};repo_b64={};branch_b64={};agent_b64={};title_b64={};ports=3000,8080\u{7}",
            STANDARD.encode("/home/notes/Projects/taskers"),
            STANDARD.encode("taskers"),
            STANDARD.encode("main"),
            STANDARD.encode("codex"),
            STANDARD.encode("codex · taskers"),
        );

        let frames = parse_signal_frames(&output);

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].kind, SignalKind::Metadata);
        let metadata = frames[0].metadata.as_ref().expect("metadata snapshot");
        assert_eq!(
            metadata.cwd.as_deref(),
            Some("/home/notes/Projects/taskers")
        );
        assert_eq!(metadata.repo_name.as_deref(), Some("taskers"));
        assert_eq!(metadata.git_branch.as_deref(), Some("main"));
        assert_eq!(metadata.agent_kind.as_deref(), Some("codex"));
        assert_eq!(metadata.title.as_deref(), Some("codex · taskers"));
        assert_eq!(metadata.ports, vec![3000, 8080]);
    }
}
