//! Reading an agent's own report.
//!
//! A brief ends by asking the child for one exact last line:
//!
//! ```text
//! RESULT status=done files=src/a.ts,src/b.ts tests=pass
//! ```
//!
//! That line is a claim, not a fact, and nothing here treats it as one. It is
//! parsed so a person can see the claim next to the pane it came from.

use serde::{Deserialize, Serialize};

/// What the child says happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunStatus {
    Done,
    Failed,
    Blocked,
}

impl RunStatus {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "done" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            "blocked" => Some(Self::Blocked),
            _ => None,
        }
    }
}

/// What the child says about its own tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TestOutcome {
    Pass,
    Fail,
    #[default]
    None,
}

impl TestOutcome {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "pass" => Some(Self::Pass),
            "fail" => Some(Self::Fail),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// A parsed `RESULT` line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub status: RunStatus,
    /// Paths the child says it changed. Empty is a valid answer.
    pub files: Vec<String>,
    pub tests: TestOutcome,
}

/// Parse one line into a result, or `None` when it is not a result line.
///
/// A line without a recognisable `status=` is not a result: a child that prints
/// something close but wrong should show as "no report", not as a report of
/// unknown state.
pub fn parse_result_line(line: &str) -> Option<RunResult> {
    let text = strip_ansi(line);
    let rest = text.trim().strip_prefix("RESULT")?;

    let mut status: Option<RunStatus> = None;
    let mut files: Vec<String> = Vec::new();
    let mut tests: Option<TestOutcome> = None;

    for token in rest.split_whitespace() {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        match key {
            "status" => status = RunStatus::parse(value),
            "files" => {
                files = value
                    .split(',')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            "tests" => tests = TestOutcome::parse(value),
            _ => {}
        }
    }

    Some(RunResult {
        status: status?,
        files,
        tests: tests.unwrap_or_default(),
    })
}

/// Finds result lines in a stream of terminal output.
///
/// Output arrives in arbitrary chunks, so a line can be split across two reads.
/// Incomplete trailing text is held until the newline that ends it arrives.
#[derive(Default)]
pub struct ResultScanner {
    pending: String,
}

impl ResultScanner {
    /// A result line is short; anything longer is ordinary output.
    const MAX_PENDING: usize = 8 * 1024;

    /// Feed output. Returns the last result line completed by this chunk.
    pub fn push(&mut self, chunk: &str) -> Option<RunResult> {
        self.pending.push_str(chunk);
        let mut latest = None;

        while let Some(newline) = self.pending.find('\n') {
            let line: String = self.pending.drain(..=newline).collect();
            if let Some(result) = parse_result_line(&line) {
                latest = Some(result);
            }
        }

        if self.pending.len() > Self::MAX_PENDING {
            self.pending.clear();
        }
        latest
    }
}

/// Remove ANSI escape sequences.
///
/// An agent that colours its output would otherwise wrap the report in escapes,
/// and `strip_prefix("RESULT")` would never match.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();

    while let Some(character) = characters.next() {
        if character != '\x1b' {
            out.push(character);
            continue;
        }
        match characters.next() {
            // Control Sequence Introducer: runs to a byte in the range @..~.
            Some('[') => {
                for next in characters.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            // Operating System Command: runs to BEL, or to ESC \.
            Some(']') => {
                while let Some(next) = characters.next() {
                    if next == '\x07' {
                        break;
                    }
                    if next == '\x1b' && characters.peek() == Some(&'\\') {
                        characters.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_full_report() {
        let result = parse_result_line("RESULT status=done files=src/a.ts,src/b.ts tests=pass")
            .expect("a report");
        assert_eq!(result.status, RunStatus::Done);
        assert_eq!(result.files, ["src/a.ts", "src/b.ts"]);
        assert_eq!(result.tests, TestOutcome::Pass);
    }

    #[test]
    fn an_empty_files_field_is_not_a_file_called_empty() {
        let result = parse_result_line("RESULT status=failed files= tests=fail").expect("a report");
        assert!(result.files.is_empty());
        assert_eq!(result.status, RunStatus::Failed);
    }

    #[test]
    fn a_missing_tests_field_means_none_were_run() {
        let result = parse_result_line("RESULT status=blocked files=").expect("a report");
        assert_eq!(result.tests, TestOutcome::None);
    }

    #[test]
    fn ordinary_output_is_not_a_report() {
        assert!(parse_result_line("running the tests now").is_none());
        assert!(parse_result_line("").is_none());
        assert!(parse_result_line("RESULTING").is_none());
    }

    #[test]
    fn a_report_without_a_usable_status_is_not_a_report() {
        // Something close but wrong must not read as a report of unknown state.
        assert!(parse_result_line("RESULT files=a.ts tests=pass").is_none());
        assert!(parse_result_line("RESULT status=almost files=a.ts").is_none());
    }

    #[test]
    fn an_unknown_field_is_ignored() {
        let result =
            parse_result_line("RESULT status=done extra=whatever files=a.ts").expect("a report");
        assert_eq!(result.files, ["a.ts"]);
    }

    #[test]
    fn a_coloured_report_is_still_read() {
        let line = "\x1b[32mRESULT\x1b[0m \x1b[1mstatus=done\x1b[0m files=a.ts tests=pass";
        let result = parse_result_line(line).expect("a report");
        assert_eq!(result.status, RunStatus::Done);
        assert_eq!(result.files, ["a.ts"]);
    }

    #[test]
    fn the_scanner_reports_the_last_result_in_a_chunk() {
        let mut scanner = ResultScanner::default();
        let chunk = "RESULT status=failed files= tests=fail\nnoise\nRESULT status=done files=b tests=pass\n";
        let result = scanner.push(chunk).expect("a report");
        assert_eq!(result.status, RunStatus::Done);
        assert_eq!(result.files, ["b"]);
    }

    #[test]
    fn the_scanner_waits_for_the_newline() {
        let mut scanner = ResultScanner::default();
        assert!(scanner.push("RESULT status=do").is_none());
        assert!(scanner.push("ne files=a tests=none").is_none());
        let result = scanner.push("\n").expect("the report once the line ends");
        assert_eq!(result.status, RunStatus::Done);
    }

    #[test]
    fn the_scanner_ignores_an_unterminated_line_forever() {
        let mut scanner = ResultScanner::default();
        for _ in 0..100 {
            assert!(scanner.push(&"x".repeat(1024)).is_none());
        }
        assert!(scanner.pending.len() <= ResultScanner::MAX_PENDING);
    }
}
