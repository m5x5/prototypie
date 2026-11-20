//! Help texts (spec-public/05 AN-CLI-229): the main text word for word, one text per group and command.

use super::out::{self, Fail};

pub const MAIN: &str = "\
prototypie: work with the Prototypie app from a terminal or a coding agent.
The app must be running; this command talks to it on 127.0.0.1:3124.

Notes (feedback people pin on the app and its designs):
  prototypie notes watch [--ack] [--after CURSOR]   wait for new notes (prints the next command)
  prototypie notes list [--status S] [--all]        list notes (default: pending)
  prototypie notes show ID                          one note with its thread
  prototypie notes ack ID...                        take notes on (\"I'm on it\")
  prototypie notes reply ID TEXT                    answer in the note's thread
  prototypie notes resolve ID [--summary TEXT]      mark done
  prototypie notes dismiss ID --reason TEXT         drop, with a reason

Design jobs (the agent queue):
  prototypie queue list | show REF | claim REF
  prototypie queue deliver REF --title T --html FILE

Cards on the canvas:
  prototypie cards list [--space ID]
  prototypie cards create --space ID --title T --html FILE

Setup:
  prototypie status                                 is the app running? what is waiting?
  prototypie skill install                          install the agent skill (SKILL.md)

Options: --json  --as NAME (or PROTOTYPIE_AGENT)  --server URL (or PROTOTYPIE_SERVER)
Exit codes: 0 ok, 2 usage, 3 app not reachable, 4 not found, 5 conflict, 1 other error.
More: prototypie help notes | queue | cards | skill
";

const NOTES: &str = "\
prototypie notes: the notes people pin on the app and on designs.

Commands:
  watch     wait for new notes; prints them and the next command to run
  list      list notes (default: pending)
  show      one note with its thread
  ack       take notes on (\"I'm on it\")
  reply     answer in a note's thread
  resolve   mark a note done
  dismiss   drop a note, with a reason
  collections   list the collections notes are filed in

Run \"prototypie notes <command> --help\" for its options.
Example: prototypie notes watch --ack
";

const QUEUE: &str = "\
prototypie queue: design jobs from Prototypie's agent queue.

Commands:
  list      the jobs waiting for an agent
  show      one job with all its fields (REF is JOB#INDEX, e.g. J1abc#0)
  claim     say you work on a job (do it before you start)
  deliver   hand in the HTML design; fills the job's card and closes its note

Run \"prototypie queue <command> --help\" for its options.
Example: prototypie queue claim J1abc#0
";

const CARDS: &str = "\
prototypie cards: spaces and design cards on the canvas.

Commands:
  list      the spaces, or the cards of one space (--space ID)
  create    create cards in a space (or a new exploration)

Run \"prototypie cards <command> --help\" for its options.
Example: prototypie cards create --space s1 --title \"Compact header\" --html header.html
";

const SKILL: &str = "\
prototypie skill: the agent skill (SKILL.md) that teaches coding agents these commands.

Commands:
  install   write the skill to ~/.agents/skills (and ~/.claude/skills, ~/.codex/skills when present)
  show      print the skill

Run \"prototypie skill <command> --help\" for its options.
Example: prototypie skill install
";

/// The help of one group.
pub fn group(name: &str) -> Option<&'static str> {
    Some(match name {
        "notes" => NOTES,
        "queue" => QUEUE,
        "cards" => CARDS,
        "skill" => SKILL,
        "status" => STATUS,
        _ => return None,
    })
}

const STATUS: &str = "\
Usage: prototypie status [--json]

Is the app running? Shows the server, the notes waiting (and the cursor), the queue,
the agents that waited for notes in the last 10 minutes, and whether the skill is installed.

Exit code 3 when the app can't be reached.
Example: prototypie status
";

/// The help of one command.
pub fn command(group: &str, cmd: &str) -> Option<&'static str> {
    Some(match (group, cmd) {
        ("notes", "watch") => "\
Usage: prototypie notes watch [--after CURSOR] [--ack] [OPTIONS]

Returns at once with the notes already waiting, or waits for new ones, then prints them and
the next command (\"Next: …\"). Always continue with that command: its cursor shows each note once.

Options:
  --after CURSOR      the cursor from the last answer (default 0: what is waiting now)
  --timeout DURATION  longest wait, 1s to 300s (default 100s; 90s, 2m and 1m30s work too)
  --batch DURATION    after the first new note, wait this long for more, 0s to 60s (default 10s)
  --ack               take every returned note on at once (notes others took drop out)
  --replies           also wake when a person replies in the thread of an open note
  --location URL      only this location        --prefix URL   locations starting with URL
  --collection ID     only this collection      --tag job|implement|project-info|none
  --follow            keep watching until Ctrl-C (for people)
  --json              the answer as JSON, with \"next\"

\"No new notes\" is a normal result (exit code 0).
Example: prototypie notes watch --after 57 --timeout 25s --batch 3s
",
        ("notes", "list") => "\
Usage: prototypie notes list [OPTIONS]

Options:
  --status S          pending (default), acknowledged, resolved, dismissed, active or all
  --all               the same as --status all
  --location URL      only this location
  --prefix URL        only locations starting with URL
  --collection ID     only this collection
  --tag T             job, implement, project-info or none
  --limit N           at most N notes, 1 to 5000 (default 100)
  --json              the service's answer as JSON

Example: prototypie notes list --status active --tag job
",
        ("notes", "show") => "\
Usage: prototypie notes show ID [--json]

One note in full: where it is, what it points at, the comment and the thread.
Exit code 4 when there is no such note.
Example: prototypie notes show n7k2x9q0bz4
",
        ("notes", "ack") => "\
Usage: prototypie notes ack ID... [--message TEXT] [--force]

Takes notes on (\"I'm on it\"). Exit code 5 when another agent already took one; the other
IDs are still handled.

Options:
  --message TEXT      also add this text to the thread (signed with your name)
  --force             take it even if it was taken already

Example: prototypie notes ack n7k2x9q0bz4 --message \"On it.\"
",
        ("notes", "reply") => "\
Usage: prototypie notes reply ID TEXT...
       prototypie notes reply ID --file FILE|-

Adds a message to the note's thread; the user sees it in the app. Agent replies are signed
with your name (--as or PROTOTYPIE_AGENT).

Options:
  --file FILE|-       read the text from a file (- for stdin)
  --role agent|human  default agent

Example: prototypie notes reply n7k2x9q0bz4 \"Which of the two headers do you mean?\"
",
        ("notes", "resolve") => "\
Usage: prototypie notes resolve ID [--summary TEXT | --summary-file FILE|-]

Marks a note done. The summary goes into the thread (signed).
Example: prototypie notes resolve n7k2x9q0bz4 --summary \"Made the button 20% smaller.\"
",
        ("notes", "dismiss") => "\
Usage: prototypie notes dismiss ID --reason TEXT
       prototypie notes dismiss ID --reason-file FILE|-

Drops a note. The reason is required and goes into the thread (signed).
Example: prototypie notes dismiss n7k2x9q0bz4 --reason \"The search field was removed.\"
",
        ("notes", "collections") => "\
Usage: prototypie notes collections [--location URL] [--state active|closed]

Lists the collections notes are filed in. Read one: prototypie notes list --collection ID --all
",
        ("queue", "list") => "\
Usage: prototypie queue list [--json]

The design jobs waiting for an agent, with who claimed them.
",
        ("queue", "show") => "\
Usage: prototypie queue show REF [--base-html] [--json]

All fields of one job (REF is JOB#INDEX, e.g. J1abc#0). Large fields show only their size.

Options:
  --base-html         print only the base design's HTML, unchanged (redirect it to a file)
  --json              the whole job as JSON

Example: prototypie queue show J1abc#0 --base-html > base.html
",
        ("queue", "claim") => "\
Usage: prototypie queue claim REF

Says you work on the job (your name: --as or PROTOTYPIE_AGENT). Exit code 5 when the job is
done or cancelled.
Example: prototypie --as proto-worker-3 queue claim J1abc#0
",
        ("queue", "deliver") => "\
Usage: prototypie queue deliver REF --title TEXT --html FILE|- [OPTIONS]

Fills the job's placeholder card with your design and closes the job's note.

Options:
  --title TEXT            the card's title (required)
  --html FILE|-           the design, a complete HTML document (- reads stdin; at most 32 MiB)
  --rationale TEXT        one or two sentences on the design (default empty)
  --viewport N            card width 200 to 4000 (default: from <meta name=\"viewport\">)
  --viewport-height N     card height 200 to 4000

Exit codes: 4 the job is no longer waiting, 5 the card was deleted in the app.
Example: prototypie queue deliver J1abc#0 --title \"Bolder price block\" --html design.html
",
        ("cards", "list") => "\
Usage: prototypie cards list [--space ID] [--json]

Without --space: one line per space (id, kind, cards, path). With --space: its cards.
",
        ("cards", "create") => "\
Usage: prototypie cards create (--space ID | --near ID) --title TEXT --html FILE|- [OPTIONS]
       prototypie cards create --from FILE|- [--space ID | --near ID]
       prototypie cards create --new-exploration NAME [--parent-space ID] [--seed ID] [card options]

Options:
  --space ID          the space (a project or exploration; not the main canvas \"root\")
  --near ID           place the card next to this card
  --title TEXT        1 to 200 characters
  --html FILE|-       the design (- reads stdin)
  --rationale TEXT    at most 4000 characters
  --kind K            page, variation (default) or spoke
  --parent ID         the card it derives from
  --component         the design is one component
  --viewport N        width 200 to 4000      --viewport-height N   height 200 to 4000
  --from FILE|-       several cards: {\"spaceId\": \"…\", \"cards\": [{\"title\": \"…\", \"html\": \"…\"}]}

Example: prototypie cards create --space s1 --title \"Compact header\" --html header.html
",
        ("skill", "install") => "\
Usage: prototypie skill install [--dir DIR]... [--force] [--no-link]

Writes the skill as <DIR>/prototypie/SKILL.md. Without --dir: ~/.agents/skills, plus
~/.claude/skills and ~/.codex/skills when ~/.claude or ~/.codex exist. A SKILL.md that is not
the Prototypie skill is left alone unless --force. When `prototypie` is not on PATH, links
~/.local/bin/prototypie to this binary (--no-link: don't). The app need not run.
",
        ("skill", "show") => "\
Usage: prototypie skill show

Prints the skill (SKILL.md) built into this binary.
",
        ("status", "") => STATUS,
        _ => return None,
    })
}

/// `prototypie help [GROUP [COMMAND]]`.
pub fn show(words: &[&str]) -> Result<i32, Fail> {
    let text = match words {
        [] => Some(MAIN),
        [g] => group(g),
        [g, c, ..] => command(g, c).or_else(|| group(g)),
    };
    match text {
        Some(t) => {
            out::print(t);
            Ok(0)
        }
        None => Err(Fail::usage(format!("Unknown command \"{}\". Run \"prototypie help\" for the commands.", words.join(" ")))),
    }
}
