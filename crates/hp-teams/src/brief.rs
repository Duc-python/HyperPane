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
//! - **VERIFICATION** — how it proves it, or how it says it could not.
//! - **REPORT** — the exact last line, so the app can read the claim.

/// A brief is a prompt, not a document. Past this, something is wrong with the
/// preset rather than with the task.
pub const MAX_BRIEF_BYTES: usize = 64 * 1024;

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
         \n\
         VERIFICATION\n\
         Run whatever proves this works and read the real output, not an exit \
         code through a pipe. Say what you ran and what it printed. If you \
         cannot prove it, say so and report blocked rather than claiming it \
         works.\n\
         \n\
         REPORT\n\
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
    );

    fit(text)
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
}
