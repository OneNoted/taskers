use taskers_domain::{SignalEvent, SignalKind};

const OSC_PREFIX: &str = "\u{1b}]777;taskers;";
const BEL: char = '\u{7}';
const ST: &str = "\u{1b}\\";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSignal {
    pub kind: SignalKind,
    pub message: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct SignalStreamParser {
    pending: String,
}

impl ParsedSignal {
    pub fn into_event(self, source: impl Into<String>) -> SignalEvent {
        SignalEvent::new(source, self.kind, self.message)
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
        let mut keep_from = self.pending.len().saturating_sub(OSC_PREFIX.len());

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

        self.pending = self.pending[keep_from..].to_string();
        frames
    }
}

fn parse_frame(frame: &str) -> Option<ParsedSignal> {
    let mut kind = None;
    let mut message = None;
    let mut title = None;

    for part in frame.split(';') {
        let (key, value) = part.split_once('=')?;
        match key {
            "kind" => {
                kind = Some(match value {
                    "started" => SignalKind::Started,
                    "progress" => SignalKind::Progress,
                    "completed" => SignalKind::Completed,
                    "waiting_input" => SignalKind::WaitingInput,
                    "error" => SignalKind::Error,
                    "notification" => SignalKind::Notification,
                    _ => return None,
                });
            }
            "message" => message = Some(value.replace("%20", " ")),
            "title" => title = Some(value.replace("%20", " ")),
            _ => {}
        }
    }

    Some(ParsedSignal {
        kind: kind?,
        message,
        title,
    })
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

#[cfg(test)]
mod tests {
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
}
