//! Client side of `/api/variations` (reference `src/lib/generate.ts` readEvents): NDJSON lines → events.

use crate::types::VariationEvent;

/// Incremental reader: feed byte chunks, take complete events. Blank lines are skipped; a line that isn't a
/// known event is an error (the reference's `JSON.parse` would throw).
#[derive(Default)]
pub struct Reader {
    raw: Vec<u8>,
}

impl Reader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a chunk and return the events completed by it.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<VariationEvent>, serde_json::Error> {
        self.raw.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(nl) = self.raw.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.raw.drain(..=nl).collect();
            let text = String::from_utf8_lossy(&line);
            let text = text.trim();
            if !text.is_empty() {
                out.push(serde_json::from_str(text)?);
            }
        }
        Ok(out)
    }
}

/// The events a failed request produces on the client (`Request failed (<status>)` + done).
pub fn request_failed(status: u16) -> [VariationEvent; 2] {
    [VariationEvent::Error { index: None, message: format!("Request failed ({status})") }, VariationEvent::Done]
}

/// The events of resuming a job the server no longer has.
pub fn job_gone() -> [VariationEvent; 2] {
    [
        VariationEvent::Error { index: None, message: "The server no longer has this job (dev server restarted?)".into() },
        VariationEvent::Done,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_lines_across_chunks() {
        let mut r = Reader::new();
        assert!(r.push(b"{\"type\":\"job\",\"id\":\"J\"}\n{\"type\":\"pl").unwrap().len() == 1);
        let ev = r.push(b"an\",\"directions\":[]}\n\n{\"type\":\"done\"}\n").unwrap();
        assert_eq!(ev, vec![VariationEvent::Plan { directions: vec![] }, VariationEvent::Done]);
        assert!(r.push(b"nope\n").is_err());
        assert_eq!(request_failed(500)[0], VariationEvent::Error { index: None, message: "Request failed (500)".into() });
        assert_eq!(job_gone()[1], VariationEvent::Done);
    }
}
