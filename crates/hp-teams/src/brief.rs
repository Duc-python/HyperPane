//! The brief a team member reads.
//!
//! A child agent gets one text and nothing else: no conversation history, no
//! context from the pane next door. So the brief has to stand alone, and it has
//! to end in a line the app can read back.
//!
//! Five parts, in this order, because a brief missing any of them produces a
//! question instead of work:
//!
//! - **GOAL** — what is true when this is finished.
//! - **SCOPE** — where it may write, and what is read-only.
//! - **SPECIFICATION** — what to do, which is the role's own instruction.
//! - **WHAT EARLIER WAVES SAID** — only for a role that answers them.
//! - **VERIFICATION** — how it proves it, or how it says it could not.
//! - **REPORT** — the exact last line, so the app can read the claim.

/// A brief is a prompt, not a document. Past this, something is wrong with the
/// preset rather than with the task.
///
/// The ceiling is a Windows one: a `.cmd` shim is started through `cmd.exe`,
/// whose command line ends at 8191 characters, and the brief is one argument on
/// it. A three-reading debate brief at the old 64 KiB cap never reached the
/// judge at all - the pane said "The command line is too long." and the run
/// moved on as if the judge had reported. Seven kilobytes leaves room for the
/// program, the arguments and the quotes, and is still far more prompt than a
/// member needs.
pub const MAX_BRIEF_BYTES: usize = 7 * 1024;

/// What an earlier wave reported, for a role that is meant to answer it.
///
/// A wave is a barrier, and the point of a later role is to do something with
/// what the earlier one said: confirm it, refuse it, or build on it. The
/// findings are handed over as text because a child gets no context of its own,
/// and the path is named so it can read the whole thing.
#[derive(Debug, Clone)]
pub struct Reading<'a> {
    pub wave: u32,
    pub label: &'a str,
    /// The line the member printed about itself, if it printed one.
    pub claim: Option<&'a str>,
    /// Where it wrote what it found, when the file has something in it.
    pub path: Option<&'a str>,
    /// What is in that file, as the app read it.
    pub findings: Option<&'a str>,
}

/// Everything a brief needs to know.
#[derive(Debug, Clone)]
pub struct BriefInput<'a> {
    /// The team's title, for the child's sense of where it is.
    pub team: &'a str,
    /// The team's one-line purpose.
    pub purpose: &'a str,
    /// The role's label, which is what the person sees in the pane header.
    pub role_label: &'a str,
    /// The role's standing instruction.
    pub focus: &'a str,
    /// What the person asked this run to do.
    pub task: &'a str,
    /// Read-only roles must not write, and the brief says so plainly.
    pub read_only: bool,
    /// The directory this member is confined to, when it has one.
    pub worktree: Option<&'a str>,
    pub wave: u32,
    /// Where a reading role writes what it found, when it has somewhere to
    /// write. A RESULT line is a summary; findings are the findings.
    pub findings: Option<&'a str>,
    /// What earlier waves said, for a role whose job is to answer them.
    pub readings: &'a [Reading<'a>],
}

/// Render the brief a member reads.
pub fn write_brief(input: &BriefInput<'_>) -> String {
    let task = if input.task.trim().is_empty() {
        "No task was given: read what you need and say what you would do."
    } else {
        input.task.trim()
    };

    let scope = if input.read_only {
        "WRITE: nothing. This role reads and reports. Do not modify, create or \
         delete any file, and do not commit, push or deploy."
            .to_string()
    } else if let Some(directory) = input.worktree {
        format!(
            "WRITE: only inside {directory}. Everything else is read-only, above \
             all shared configuration (package manifests, lock files, routes, \
             schemas). Do not commit, push, deploy or release."
        )
    } else {
        "WRITE: the files this task needs. Shared configuration (package \
         manifests, lock files, routes, schemas) is read-only: if one has to \
         change, stop and report blocked. Do not commit, push, deploy or \
         release."
            .to_string()
    };

    let findings = match input.findings {
        Some(path) => format!(
            "\nWrite what you found to {path} before you finish: one finding per \
             line, each naming the file and line it is about and why it matters. \
             Write nothing else in that file. A person reads it before deciding \
             whether the work goes on, so an empty file is a valid answer and \
             an invented one is not.\n"
        ),
        None => String::new(),
    };

    let readings = render_readings(input.readings);

    let text = format!(
        "GOAL\n\
         {task}\n\
         \n\
         SCOPE\n\
         You are {role} in the team \"{team}\" ({purpose}). This is wave {wave}.\n\
         {scope}\n\
         \n\
         SPECIFICATION\n\
         {focus}\n\
         {readings}\
         \n\
         VERIFICATION\n\
         Run whatever proves this works and read the real output, not an exit \
         code through a pipe. Say what you ran and what it printed. If you \
         cannot prove it, say so and report blocked rather than claiming it \
         works.\n\
         \n\
         REPORT\n\
         {findings}\
         As your last line, print exactly this shape, one line:\n\
         RESULT status=done|failed|blocked files=<comma-separated paths you \
         changed, empty if none> tests=pass|fail|none\n",
        task = task,
        role = input.role_label,
        team = input.team,
        purpose = input.purpose,
        wave = input.wave,
        scope = scope,
        focus = input.focus.trim(),
        readings = readings,
        findings = findings,
    );

    fit(text)
}

/// The most of a brief that earlier waves' findings may take.
///
/// The report line at the end is how the app reads a member's claim, so a pile
/// of findings must not push it out of a brief that is cut at the cap. Half the
/// brief is the most the section may use; the role's own instruction and the
/// report line get the rest.
const MAX_READINGS_BYTES: usize = MAX_BRIEF_BYTES / 2;

/// What a cut section says, so nobody mistakes a shortened finding for all of
/// one. The member's file is named above it.
const CUT_NOTE: &str = "  [cut here: the rest is in the file named above]\n";

/// The section a role reads before it answers an earlier wave.
///
/// Empty when there is nothing to answer, so a first wave's brief is unchanged.
fn render_readings(readings: &[Reading<'_>]) -> String {
    let spoken: Vec<&Reading<'_>> = readings
        .iter()
        .filter(|reading| reading.findings.is_some())
        .collect();
    if spoken.is_empty() {
        return String::new();
    }

    let mut section = String::from(
        "\nWHAT EARLIER WAVES SAID\n\
         These are claims by other members, not facts the app checked. Answer \
         them: name the claim you looked at, say what you found, and say where \
         you disagree. Agreeing in one line is enough - do not restate it.\n",
    );

    // Equal shares, because a verbose member must not be the only one the next
    // wave can read.
    let share = MAX_READINGS_BYTES / spoken.len();

    for reading in spoken {
        let claim = reading
            .claim
            .map(|claim| format!(" - {claim}"))
            .unwrap_or_default();
        let mut block = format!("\n- Wave {}, {}{}:\n", reading.wave, reading.label, claim);
        if let Some(path) = reading.path {
            block.push_str(&format!("  full file: {path}\n"));
        }
        for line in reading.findings.unwrap_or_default().lines() {
            block.push_str("  ");
            block.push_str(line);
            block.push('\n');
        }
        section.push_str(&clip(block, share));
    }

    section
}

/// Hold one member's section to its share, on a line boundary, and say that it
/// was cut rather than letting the brief's own cap do it silently at the end.
fn clip(mut block: String, limit: usize) -> String {
    if block.len() <= limit {
        return block;
    }

    let mut end = limit.saturating_sub(CUT_NOTE.len());
    while end > 0 && !block.is_char_boundary(end) {
        end -= 1;
    }
    let end = block[..end].rfind('\n').map(|at| at + 1).unwrap_or(end);
    block.truncate(end);
    block.push_str(CUT_NOTE);
    block
}

/// Strip what a terminal would choke on, and hold the size to the cap.
fn fit(text: String) -> String {
    // Control characters other than the newline and the tab have no meaning in
    // a prompt and can only cause trouble in whatever reads it back.
    let cleaned: String = text
        .chars()
        .filter(|character| !character.is_control() || *character == '\n' || *character == '\t')
        .collect();

    if cleaned.len() <= MAX_BRIEF_BYTES {
        return cleaned;
    }

    // Cut on a character boundary, and say that it was cut rather than
    // pretending the brief was this short all along.
    let mut end = MAX_BRIEF_BYTES;
    while end > 0 && !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[truncated: this brief was longer than {MAX_BRIEF_BYTES} bytes]\n",
        &cleaned[..end]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>() -> BriefInput<'a> {
        BriefInput {
            team: "Build a feature",
            purpose: "Plans, builds and reviews a change.",
            role_label: "Builder",
            focus: "Implement the approved plan in your own worktree.",
            task: "Add a retry to the upload path.",
            read_only: false,
            worktree: None,
            wave: 1,
            findings: None,
            readings: &[],
        }
    }

    #[test]
    fn a_brief_has_all_five_parts() {
        let brief = write_brief(&input());
        for part in ["GOAL", "SCOPE", "SPECIFICATION", "VERIFICATION", "REPORT"] {
            assert!(brief.contains(part), "missing {part}:\n{brief}");
        }
    }

    #[test]
    fn a_brief_names_the_task_the_team_and_the_role() {
        let brief = write_brief(&input());
        assert!(brief.contains("Add a retry to the upload path."), "{brief}");
        assert!(brief.contains("Build a feature"), "{brief}");
        assert!(brief.contains("You are Builder"), "{brief}");
        assert!(brief.contains("wave 1"), "{brief}");
    }

    #[test]
    fn a_brief_ends_with_the_line_the_app_reads_back() {
        let brief = write_brief(&input());
        let last = brief.lines().last().expect("a last line");
        assert!(
            last.starts_with("RESULT status=done|failed|blocked"),
            "{last}"
        );
        assert!(last.contains("files="), "{last}");
        assert!(last.contains("tests=pass|fail|none"), "{last}");
    }

    #[test]
    fn an_earlier_wave_is_put_in_front_of_the_one_that_answers_it() {
        let reading = Reading {
            wave: 1,
            label: "Logic reviewer",
            claim: Some("done, tests pass"),
            path: Some("/tmp/findings/p3.md"),
            findings: Some("CRITICAL src/a.ts:12 - the retry loop never exits"),
        };
        let brief = write_brief(&BriefInput {
            wave: 2,
            readings: &[reading],
            ..input()
        });

        assert!(brief.contains("WHAT EARLIER WAVES SAID"), "{brief}");
        assert!(
            brief.contains("Wave 1, Logic reviewer - done, tests pass"),
            "{brief}"
        );
        assert!(brief.contains("full file: /tmp/findings/p3.md"), "{brief}");
        assert!(brief.contains("CRITICAL src/a.ts:12"), "{brief}");

        // After the role's own instruction, and before the report it writes.
        let spec = brief.find("SPECIFICATION").expect("spec");
        let said = brief.find("WHAT EARLIER WAVES SAID").expect("readings");
        let report = brief.find("REPORT").expect("report");
        assert!(spec < said && said < report, "{brief}");
    }

    #[test]
    fn a_three_case_debate_fits_a_windows_command_line() {
        // The judge reads the two cases and the answer: three findings files of
        // a couple of kilobytes each. The brief is one argument on a `cmd.exe`
        // command line, which ends at 8191 characters - at the old cap this
        // brief was cut by cmd itself, the judge never started, and the run
        // reported itself finished.
        let findings = "x".repeat(3000);
        let readings: Vec<Reading> = (1..=3)
            .map(|wave| Reading {
                wave,
                label: "Case",
                claim: Some("done, no files, no tests"),
                path: Some("C:\\Users\\someone\\findings\\p9.md"),
                findings: Some(findings.as_str()),
            })
            .collect();

        let brief = write_brief(&BriefInput {
            wave: 4,
            readings: &readings,
            ..input()
        });

        assert!(brief.len() < 8000, "{}", brief.len());
        assert!(brief.contains("RESULT status="), "the report line was cut");
        assert!(brief.contains("cut here"), "a cut finding says so");
    }

    #[test]
    fn a_member_that_wrote_nothing_is_not_a_section() {
        let quiet = Reading {
            wave: 1,
            label: "Quiet",
            claim: Some("done"),
            path: None,
            findings: None,
        };
        let brief = write_brief(&BriefInput {
            wave: 2,
            readings: &[quiet],
            ..input()
        });
        assert!(!brief.contains("WHAT EARLIER WAVES SAID"), "{brief}");
    }

    #[test]
    fn a_wall_of_findings_cannot_push_out_the_line_the_app_reads_back() {
        // The app reads a claim from the brief's last line, so a cut at the
        // brief's cap must never be the reason a member's RESULT is lost.
        let long = "a finding that goes on. ".repeat(20_000);
        let first = Reading {
            wave: 1,
            label: "First",
            claim: None,
            path: Some("/tmp/first.md"),
            findings: Some(&long),
        };
        let second = Reading {
            wave: 1,
            label: "Second",
            claim: None,
            path: Some("/tmp/second.md"),
            findings: Some(&long),
        };
        let brief = write_brief(&BriefInput {
            wave: 2,
            readings: &[first, second],
            ..input()
        });

        // Each member keeps its section and its way to the whole file, rather
        // than one long finding silencing the other.
        assert!(brief.contains("full file: /tmp/first.md"), "{brief}");
        assert!(brief.contains("full file: /tmp/second.md"), "{brief}");
        assert_eq!(brief.matches("[cut here").count(), 2, "{}", brief.len());

        let last = brief.lines().last().expect("a last line");
        assert!(
            last.starts_with("RESULT status=done|failed|blocked"),
            "{last}"
        );
        assert!(brief.len() <= MAX_BRIEF_BYTES, "{}", brief.len());
    }

    #[test]
    fn a_read_only_role_is_told_so_in_plain_words() {
        let brief = write_brief(&BriefInput {
            read_only: true,
            worktree: Some("/tmp/worktrees/agent-abc"),
            ..input()
        });
        assert!(brief.contains("WRITE: nothing"), "{brief}");
        assert!(
            !brief.contains("/tmp/worktrees/agent-abc"),
            "a read-only role should not be handed a write scope:\n{brief}"
        );
    }

    #[test]
    fn a_writer_locked_to_a_worktree_is_told_where_it_may_write() {
        let brief = write_brief(&BriefInput {
            worktree: Some("/tmp/worktrees/agent-abc"),
            ..input()
        });
        assert!(
            brief.contains("only inside /tmp/worktrees/agent-abc"),
            "{brief}"
        );
    }

    #[test]
    fn a_writer_without_a_worktree_is_told_what_not_to_touch() {
        let brief = write_brief(&input());
        assert!(brief.contains("Shared configuration"), "{brief}");
        assert!(brief.contains("report blocked"), "{brief}");
    }

    #[test]
    fn an_empty_task_asks_for_a_plan_instead_of_inventing_one() {
        let brief = write_brief(&BriefInput {
            task: "   ",
            ..input()
        });
        assert!(brief.contains("No task was given"), "{brief}");
    }

    #[test]
    fn control_characters_are_stripped() {
        let brief = write_brief(&BriefInput {
            focus: "work\x07hard\x1b[31m",
            ..input()
        });
        assert!(!brief.contains('\x07'), "bell survived");
        assert!(!brief.contains('\x1b'), "escape survived");
        assert!(brief.contains("workhard"), "{brief}");
    }

    #[test]
    fn a_brief_longer_than_the_cap_is_cut_and_says_so() {
        let long = "x".repeat(MAX_BRIEF_BYTES + 500);
        let brief = write_brief(&BriefInput {
            focus: &long,
            ..input()
        });
        assert!(brief.len() < MAX_BRIEF_BYTES + 200, "{}", brief.len());
        assert!(brief.contains("[truncated"), "the cut is not announced");
    }

    #[test]
    fn a_brief_at_the_cap_is_left_alone() {
        // The cap is on the rendered text, so a focus that fits stays intact.
        let focus = "y".repeat(1000);
        let brief = write_brief(&BriefInput {
            focus: &focus,
            ..input()
        });
        assert!(!brief.contains("[truncated"));
        assert!(brief.contains(&focus));
    }

    #[test]
    fn a_reader_is_told_where_to_write_what_it_found() {
        let brief = write_brief(&BriefInput {
            findings: Some("/tmp/findings/p3.md"),
            ..input()
        });
        assert!(brief.contains("/tmp/findings/p3.md"), "{brief}");
        assert!(brief.contains("one finding per line"), "{brief}");
        assert!(
            brief.contains("an empty file is a valid answer"),
            "a reader must not feel obliged to invent findings:\n{brief}"
        );
    }

    #[test]
    fn a_brief_with_nothing_to_write_to_says_nothing_about_findings() {
        let brief = write_brief(&input());
        assert!(!brief.contains("found to"), "{brief}");
        assert!(!brief.contains("empty file"), "{brief}");
    }
}
