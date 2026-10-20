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
        // A border or a full stop can sit against the word.
        match value.trim_end_matches(|c: char| !c.is_alphanumeric()) {
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
        match value.trim_end_matches(|c: char| !c.is_alphanumeric()) {
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
    parse_fields(result_body(&text)?)
}

/// The text after `RESULT` on a line that starts a report.
///
/// A terminal draws its own frame around what an agent says - a box border, a
/// quote marker, a bullet. Those lead the line, so the report is found after
/// them, but only when everything before it is punctuation: prose that happens
/// to say "RESULT" mid-sentence stays prose.
fn result_body(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let start = trimmed.find("RESULT")?;
    if !trimmed[..start].chars().all(|c| !c.is_alphanumeric()) {
        return None;
    }
    match trimmed[start + "RESULT".len()..].chars().next() {
        None => Some(""),
        Some(c) if c.is_whitespace() => Some(trimmed[start + "RESULT".len()..].trim()),
        // `RESULTING`, `RESULTS:` - words, not reports.
        Some(_) => None,
    }
}

/// Read the `key=value` fields of a report.
fn parse_fields(rest: &str) -> Option<RunResult> {
    let mut status: Option<RunStatus> = None;
    let mut files: Vec<String> = Vec::new();
    let mut tests: Option<TestOutcome> = None;

    for token in rest.split_whitespace() {
        let (key, value) = field(token);
        match key {
            "status" => status = RunStatus::parse(value),
            "files" => {
                files = value
                    .split(',')
                    .map(|entry| entry.trim_matches(is_frame).trim())
                    // "none" is how a report says it changed nothing; it is not
                    // a file called none.
                    .filter(|entry| !entry.is_empty() && !is_none_word(entry))
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

/// One `key=value` field, with the frame's punctuation taken off its edges.
///
/// A box border or a trailing quote is part of the drawing, not of the answer.
fn field(token: &str) -> (&str, &str) {
    let clean = token.trim_matches(is_frame);
    match clean.split_once('=') {
        Some((key, value)) => (key, value.trim_matches(is_frame)),
        None => (clean, ""),
    }
}

/// The word a report uses for an empty field.
fn is_none_word(value: &str) -> bool {
    value.eq_ignore_ascii_case("none") || value == "-"
}

/// Characters a terminal uses to frame text, which no field needs at its edge.
fn is_frame(c: char) -> bool {
    !c.is_alphanumeric() && !matches!(c, '/' | '\\' | '.' | '_' | '-' | '=' | ',')
}

/// Finds result lines in a stream of terminal output.
///
/// Output arrives in arbitrary chunks, so a line can be split across two reads.
/// Incomplete trailing text is held until the newline that ends it arrives.
#[derive(Default)]
pub struct ResultScanner {
    pending: String,
    /// A report that has started but does not have everything a report needs.
    ///
    /// Agents print the report as a block - `RESULT` and then one field per
    /// line, often inside the box their own UI draws - even when they were asked
    /// for one line. Reading line by line found nothing in that, so the lines
    /// are kept here until the report is complete or something that is not a
    /// field ends it.
    block: Option<String>,
}

impl ResultScanner {
    /// A result line is short; anything longer is ordinary output.
    const MAX_PENDING: usize = 8 * 1024;
    /// A report is a handful of short fields. Anything longer is not one.
    const MAX_BLOCK: usize = 1024;

    /// Feed output. Returns the last result completed by this chunk.
    pub fn push(&mut self, chunk: &str) -> Option<RunResult> {
        self.pending.push_str(chunk);
        let mut latest = None;

        while let Some(newline) = self.pending.find('\n') {
            let raw: String = self.pending.drain(..=newline).collect();
            let line = strip_ansi(&raw);
            let trimmed = line.trim();

            if let Some(block) = self.block.as_mut() {
                if is_field_line(trimmed) {
                    block.push(' ');
                    block.push_str(trimmed);
                    if is_complete(block) || block.len() > Self::MAX_BLOCK {
                        latest = self.finish().or(latest);
                    }
                    continue;
                }
                // Something that is not a field ends the report: a blank line,
                // a prompt, the next line of the conversation.
                latest = self.finish().or(latest);
            }

            if let Some(rest) = result_body(trimmed) {
                if is_complete(rest) {
                    latest = parse_fields(rest).or(latest);
                } else {
                    self.block = Some(rest.to_string());
                }
            }
        }

        if self.pending.len() > Self::MAX_PENDING {
            self.pending.clear();
        }
        latest
    }

    /// End the open report, if there is one.
    fn finish(&mut self) -> Option<RunResult> {
        parse_fields(&self.block.take()?)
    }
}

/// Whether a line is one more field of a report that is already open.
fn is_field_line(line: &str) -> bool {
    !line.is_empty() && line.contains('=') && !line.contains("RESULT")
}

/// Whether a report has every part the app asks for.
///
/// Status alone is not enough to stop reading: the block form reaches it first
/// and would otherwise be reported with no files and no tests. A report that
/// never gets all three still ends at the next line that is not a field.
fn is_complete(block: &str) -> bool {
    block.contains("status=") && block.contains("files=") && block.contains("tests=")
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
    fn a_report_that_changed_nothing_names_no_files() {
        // The instruction offers "none" for an empty list; that is not a path.
        let result =
            parse_result_line("RESULT status=done files=none tests=none").expect("a report");
        assert!(result.files.is_empty());
        assert_eq!(result.tests, TestOutcome::None);
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
    fn a_box_the_terminal_drew_is_not_part_of_the_answer() {
        // A CLI draws its own frame; the frame is not a field.
        let result =
            parse_result_line("│ RESULT status=done files=a.ts │ tests=pass │").expect("a report");
        assert_eq!(result.status, RunStatus::Done);
        assert_eq!(result.files, ["a.ts"]);
        assert_eq!(result.tests, TestOutcome::Pass);
    }

    #[test]
    fn prose_that_mentions_a_result_is_not_a_report() {
        assert!(parse_result_line("The RESULT status=done is what we want").is_none());
        assert!(parse_result_line("RESULTS: all green").is_none());
    }

    #[test]
    fn a_report_written_as_a_block_is_still_a_report() {
        // What a real opencode run printed when asked for one line: the word,
        // then one field per line, inside its own box.
        let mut scanner = ResultScanner::default();
        assert!(scanner.push("│ RESULT │\n").is_none());
        assert!(scanner.push("│ status=done │\n").is_none());
        assert!(scanner.push("│ files=a.ts,b.ts │\n").is_none());

        let result = scanner.push("│ tests=pass │\n").expect("a report");
        assert_eq!(result.status, RunStatus::Done);
        assert_eq!(result.files, ["a.ts", "b.ts"]);
        assert_eq!(result.tests, TestOutcome::Pass);
    }

    #[test]
    fn a_block_that_never_reaches_a_file_list_still_reports() {
        let mut scanner = ResultScanner::default();
        assert!(scanner.push("RESULT status=blocked\n").is_none());
        assert!(scanner.push("reason=no network\n").is_none());
        // A blank line is what ends the report.
        let result = scanner.push("\n").expect("a report");
        assert_eq!(result.status, RunStatus::Blocked);
        assert!(result.files.is_empty());
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
