#!/usr/bin/env python3
"""Tracked-exclusions lint — mechanical enforcement of
`~/.claude/rules/exclusions.md`: every test- or coverage-suppression marker
must carry a `tracked: <repo>#<n>` or `adr: <path>` reference on the same
line or in a directly adjacent comment (window N-3 .. N+1, inclusive).

The *why* lives in `~/.claude/rules/exclusions.md` ("Test & Coverage
Exclusions" rule). Without this gate the rule was documented but
unenforced — documentation rots; CI doesn't.

Same mechanical-enforcement pattern as crap4rs's `scripts/bdd-tracked-lint.py`
and `scripts/mutants-skip-lint.py`. Single source of truth in this script;
`.github/workflows/bdd-tracked-lint.yml` and `lefthook.yml` both invoke it.

Scope (acceptance criteria from dry-rs#22):

* Rust attrs: ``#[ignore]`` / ``#[ignore = "..."]`` /
  ``#[cfg(skip_in_ci)]`` (and other ``#[cfg(skip_*)]`` shapes)
* JS/TS test-runner skips: ``it.skip(...)``, ``describe.skip(...)``,
  ``xtest(...)``, ``xit(...)``, ``test.skip(...)``
* CI cargo invocations: ``--exclude <crate>`` flags in workflow YAML
  run-lines
* Config files: ``exclude``, ``excluded_files``, ``skip`` array
  assignments in known config filenames (``crap4rs.toml``,
  ``mutants.toml``, ``.cargo/mutants.toml``, ``vitest.config.{ts,js}``,
  ``dependency-cruiser.cjs``, ``eslint.config.{js,cjs,mjs,ts}``)
* Comment markers: ``TODO: re-enable`` / ``FIXME: skip`` near test code

Tracked-comment shape (per the rule's "Format" section):

* ``tracked: <repo>#<digits>`` — repo segment is lowercase alphanumeric
  with hyphens (e.g. ``mokumo``, ``dry-rs``, ``crap4rs``, ``scrap-rs``)
* ``adr: <relative-path>`` — non-empty path, typically under
  ``ops/decisions/`` or ``docs/`` (the lint accepts any non-whitespace
  path; the *existence* of the ADR file is intentionally NOT checked
  because ADRs in this org live in a private ops vault outside the
  public-repo tree).

This first-pass enforcer is **deliberately relaxed** on the marker's
reason-field: the rule's "Format" section requires a ``—`` (em-dash)
separator + non-empty one-line reason, but ``MARKER_RE`` only checks
the ``tracked: repo#N`` / ``adr: path`` prefix. Rationale: catching
the load-bearing case (marker entirely absent) covers the
high-frequency failure mode; reason-field shape is a tighter audit
that fits a future PR if the loose enforcement misses anything real.
crap4rs's lint is strict on this; dry-rs may converge later.

ADJACENT-WINDOW semantics: the marker may appear on the same line as
the suppression OR in any of the 3 lines above OR the 1 line below.
The window size is conservative — comment-above-then-code is the
canonical Rust/TOML/YAML shape; trailing-comment (same-line or
+1 line) covers JS/TS object-array trailing comments.

Out of scope (deliberate, documented):

* **Routing-shaped workflow ``if:`` conditions** — the rule lists
  ``if:`` conditions "that disable a job during normal CI." Routing
  guards (``if: github.event_name == 'pull_request'``,
  ``if: github.ref == 'refs/heads/main'``,
  ``if: needs.X.outputs.Y == 'true'``) are NOT in scope — they route
  jobs to the right context, they don't disable. We DO flag the
  hard-disable shapes ``if: false`` / ``if: never`` /
  ``if: ${{ false }}`` (the ``workflow-if-hard-disable`` pattern below)
  as the unambiguous "this job is shut off" signal. Soft-disable
  patterns (mixed-boolean expressions evaluating false) are still
  out of scope: a precise classifier would need expression-language
  evaluation. If a real soft-disable lands unmarked, raise an issue +
  extend the lint then.
* **Commented-out test bodies without TODO/FIXME markers** — the rule
  mentions "commented-out test bodies or ``if (false)`` early returns"
  but the only mechanical signal the rule actually defines is
  ``TODO: re-enable`` / ``FIXME: skip``. A broader heuristic ("any
  ``//`` block inside a test fn") would either false-positive on
  legitimate comments or require a Rust parser. Out of scope; the
  ``TODO/FIXME`` markers are the supported tripwire.
* **``Cargo.toml`` ``[package].exclude`` / workspace exclude** — these
  are publish-scope (which files ship in a crate tarball), not
  test-coverage suppressions. We pin the config-file scan to a known
  filename allowlist that explicitly excludes ``Cargo.toml`` /
  ``Cargo.lock``.
* **``deny.toml`` license/advisory exemptions** — supply-chain policy,
  not test suppression. Same filename-allowlist mechanism applies.
* **``.gitignore`` / ``.ignore``** — VCS scope, not test scope.

If any of these gaps produces a real regression, extend the lint.

Self-reference: this script's source contains every suppression-marker
literal it scans for. Two defenses keep it from false-positiving on
itself:

1. The script's own path (``scripts/tracked-lint.py``) is added to a
   ``HARD_SKIP_PATHS`` set, so the file walker never visits it.
2. The path allowlist (``SCAN_GLOBS``) doesn't include ``scripts/``
   anyway; defense in depth.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

# Directories scanned recursively (relative to repo root). Anything
# outside this set is ignored: keeps the lint scoped to project source
# and out of generated artifacts (``target/``), vendored dependencies,
# or top-level meta files like ``Cargo.toml``.
SCAN_GLOBS: list[str] = [
    "crates/**/*.rs",
    "crates/**/*.toml",
    "crates/**/*.yml",
    "crates/**/*.yaml",
    "crates/**/*.feature",
    "crates/**/*.ts",
    "crates/**/*.tsx",
    "crates/**/*.js",
    "crates/**/*.jsx",
    "crates/**/*.mjs",
    "crates/**/*.cjs",
    "tests/**/*.rs",
    "tests/**/*.ts",
    "tests/**/*.tsx",
    "tests/**/*.js",
    "tests/**/*.jsx",
    "tests/**/*.mjs",
    "tests/**/*.cjs",
    ".github/workflows/*.yml",
    ".github/workflows/*.yaml",
    ".github/actions/**/*.yml",
    ".github/actions/**/*.yaml",
    ".cargo/*.toml",
]

# Known top-level config filenames that may legitimately carry
# ``exclude`` / ``skip`` arrays in a test-suppression sense. Each entry
# is a path-relative-to-root literal. Cargo.toml is INTENTIONALLY OMITTED
# (its ``exclude = [...]`` is publish-scope, not test-scope).
KNOWN_CONFIG_FILES: list[str] = [
    "crap4rs.toml",
    "scrap-rs.toml",
    "scrap4rs.toml",
    "mutants.toml",
    ".cargo/mutants.toml",
    "vitest.config.ts",
    "vitest.config.js",
    "dependency-cruiser.cjs",
    "eslint.config.js",
    "eslint.config.cjs",
    "eslint.config.mjs",
    "eslint.config.ts",
]

# Hard skips — paths the walker MUST NOT visit even if a future
# ``SCAN_GLOBS`` entry would include them. Belt-and-suspenders against
# the lint catching its own source.
HARD_SKIP_PATHS: frozenset[str] = frozenset(
    {
        "scripts/tracked-lint.py",
    }
)

# Adjacent-comment search window. For a hit at line N, scan lines
# [N - PRE_WINDOW, N + POST_WINDOW] for a tracked/adr marker. PRE is
# wider because comment-above-then-code is the canonical Rust/TOML/YAML
# shape (block comments span multiple lines). POST = 1 covers
# trailing-comment patterns in JS/TS arrays.
PRE_WINDOW: int = 3
POST_WINDOW: int = 1

# ---------------------------------------------------------------------------
# Marker regex
# ---------------------------------------------------------------------------

# Repo segment is lowercase alphanumeric + hyphens, matching the org's
# repo-naming convention (``mokumo``, ``dry-rs``, ``crap4rs``,
# ``scrap-rs``, ``ops``, etc.). Issue number is one or more digits.
# ADR path is any non-whitespace string; existence not checked because
# ops/decisions/ lives in a private vault outside this repo's tree.
MARKER_RE = re.compile(
    r"(?:tracked:\s*[a-z0-9][a-z0-9_-]*#\d+|adr:\s*\S+)"
)

# ---------------------------------------------------------------------------
# Suppression patterns
# ---------------------------------------------------------------------------
#
# Each entry: (pattern_name, compiled_regex, applicable_file_suffixes).
# `applicable_file_suffixes` is a tuple of lower-case extensions (with
# leading dot) the pattern applies to; empty tuple means "all scanned
# files." Patterns are line-anchored where possible to keep the regex
# straightforward and the line-numbering exact.
#
# RUST_IGNORE matches both ``#[ignore]`` and ``#[ignore = "..."]``;
# leading whitespace allowed. Doc-attribute ``//! #[ignore]`` and
# string-literal occurrences are deliberately not excluded — that's
# the marker's whole point: any literal occurrence must carry a
# tracking reference, including ones in docs (where the test-rule
# rationale still applies).
#
# The patterns are SPLIT-STRING-CONSTRUCTED so this very source file
# does not contain the literal marker text the lint scans for. That's
# the rule's "no marker without tracking issue" applied to the lint's
# own source — without the split, this script would catch itself.
# See module docstring "Self-reference" section.

_HASH = "#"
_LBRACK = "["

# Match the suppression marker anywhere within the attribute brackets
# (not just at the start of the line) so the lint catches:
#
#   * ``#[cfg_attr(target_os = "linux", ignore)]`` — conditional ignore
#   * ``#[some_attr] #[ignore]`` — second attribute on the same line
#   * ``#[cfg(all(unix, skip_in_ci))]`` — nested skip predicate
#
# The ``[^\]]*`` negated-set keeps each match scoped to a single
# attribute (can't span across multiple ``#[...]`` blocks). The leading
# ``#[`` literals stay required so identifiers like ``my_ignore_fn``
# or ``cfg_table`` don't match.
RUST_IGNORE_RE = re.compile(
    rf"{re.escape(_HASH)}{re.escape(_LBRACK)}[^\]]*\bignore\b[^\]]*\]"
)
RUST_CFG_SKIP_RE = re.compile(
    rf"{re.escape(_HASH)}{re.escape(_LBRACK)}cfg\s*\([^\]]*\b(skip_in_ci|skip_[a-z_]+)\b"
)

# ``it.skip(``, ``describe.skip(``, ``test.skip(``, ``xtest(``,
# ``xit(``, ``xdescribe(``. ``.skip()`` may chain off the runner
# value (``vitest.skip``) or the test fn (``test.skip``); both shapes
# match.
JS_SKIP_RE = re.compile(
    r"\b(?:it|describe|test|context|suite)\.(?:skip|todo)\s*\(|\b(?:xit|xtest|xdescribe|xcontext|xsuite)\s*\("
)

# ``--exclude <name>`` as a free-standing CLI flag. Allow either
# ``--exclude foo`` (space-separated) or ``--exclude=foo`` (equals
# form). Anchor to a word boundary on both ends so accidental
# substrings (``--exclude-hidden``, ``--cluster-exclude``) don't match.
CARGO_EXCLUDE_RE = re.compile(r"(?<![A-Za-z0-9_-])--exclude(?:=|\s+)\S+")

# TOML/YAML/cjs array assignments. Two shapes matter:
#   * ``exclude = [`` / ``excluded_files = [`` / ``skip = [``
#     (TOML, .cjs, .ts module exports)
#   * ``exclude:`` / ``excluded_files:`` / ``skip:`` (YAML key,
#     followed by either an inline ``[...]`` or a block list ``-``
#     introducer; this lint flags the key line — the marker can sit
#     on the same line OR adjacent comments).
#
# Keep the regex strict to avoid catching identifiers like
# ``excluded_files_count`` or ``skip_count``. ``\b`` doesn't behave
# uniformly on dotted/dashed identifiers, so we use a leading-line
# anchor + explicit name list + lookahead for the assignment glyph.
CONFIG_ARRAY_RE = re.compile(
    r"^[ \t]*(exclude|excluded_files|skip|excludePaths)\s*(=|:)\s*(\[|$)"
)

# TODO: re-enable / FIXME: skip markers in any comment context.
# Anchored after a comment-introducer character (``#``, ``//``) so
# stray identifiers like ``handle_todo_reenable`` don't match.
COMMENT_TODO_RE = re.compile(
    r"(?:#|//)[^\n]*\b(?:TODO|FIXME)\s*[:\-]?\s*(?:re-?enable|skip)\b",
    re.IGNORECASE,
)

# Workflow ``if:`` hard-disable patterns. Only the unambiguous "this
# job/step is shut off" shapes match — routing guards
# (``if: github.event_name == ...``) deliberately do NOT match.
# Forms covered:
#   * ``if: false``
#   * ``if: never``
#   * ``if: ${{ false }}`` (with optional inner whitespace)
# Anchored to a line-leading ``if:`` so values inside ``run:`` blocks
# don't trip it.
WORKFLOW_IF_DISABLE_RE = re.compile(
    r"^[ \t]*if:\s*(?:false|never|\$\{\{\s*false\s*\}\})\s*(?:#.*)?$",
    re.IGNORECASE,
)


@dataclass
class Pattern:
    name: str
    regex: re.Pattern[str]
    description: str
    # File suffixes this pattern applies to (empty = all scanned files).
    suffixes: tuple[str, ...]


PATTERNS: list[Pattern] = [
    Pattern(
        name="rust-ignore",
        regex=RUST_IGNORE_RE,
        description=(
            "Rust #[ignore] / #[ignore = \"...\"] attribute"
        ),
        suffixes=(".rs",),
    ),
    Pattern(
        name="rust-cfg-skip",
        regex=RUST_CFG_SKIP_RE,
        description=(
            "Rust #[cfg(skip_in_ci)] / #[cfg(skip_*)] conditional gate"
        ),
        suffixes=(".rs",),
    ),
    Pattern(
        name="js-test-skip",
        regex=JS_SKIP_RE,
        description=(
            "JS/TS test-runner skip: it.skip / describe.skip / "
            "test.skip / xit / xtest / xdescribe"
        ),
        suffixes=(
            ".ts",
            ".tsx",
            ".js",
            ".jsx",
            ".mjs",
            ".cjs",
        ),
    ),
    Pattern(
        name="cargo-exclude",
        regex=CARGO_EXCLUDE_RE,
        description="--exclude <crate> flag in CI invocation",
        suffixes=(".yml", ".yaml"),
    ),
    Pattern(
        name="config-array",
        regex=CONFIG_ARRAY_RE,
        description="exclude/excluded_files/skip array assignment",
        # Config-array patterns are scoped to the KNOWN_CONFIG_FILES
        # list via a path check in `scan_file()` — the suffix tuple is
        # advisory only (matches the file extensions those configs use).
        suffixes=(".toml", ".cjs", ".js", ".mjs", ".ts", ".yml", ".yaml"),
    ),
    Pattern(
        name="comment-todo",
        regex=COMMENT_TODO_RE,
        description="TODO: re-enable / FIXME: skip comment marker",
        suffixes=(),  # all scanned files
    ),
    Pattern(
        name="workflow-if-hard-disable",
        regex=WORKFLOW_IF_DISABLE_RE,
        description=(
            "Workflow `if:` hard-disable (if: false / never / "
            "${{ false }}) — see module docstring for routing-vs-disable"
        ),
        suffixes=(".yml", ".yaml"),
    ),
]


# ---------------------------------------------------------------------------
# Scan logic
# ---------------------------------------------------------------------------


@dataclass
class Hit:
    file: Path
    line_no: int  # 1-indexed
    line_text: str
    pattern_name: str
    pattern_description: str


def _line_window(lines: list[str], line_no: int) -> str:
    """Return joined text of the ``[line_no - PRE_WINDOW, line_no + POST_WINDOW]``
    window (inclusive, 1-indexed). Out-of-range indices are clipped.
    Joining means the marker regex sees a single string and works
    regardless of where in the window the marker lives."""
    lo = max(1, line_no - PRE_WINDOW)
    hi = min(len(lines), line_no + POST_WINDOW)
    # `lines` is 0-indexed; convert.
    return "\n".join(lines[lo - 1 : hi])


def _is_known_config(rel_path: Path) -> bool:
    """Return True if ``rel_path`` (POSIX-style, repo-relative) is one
    of the known suppression-bearing config filenames. The
    ``config-array`` pattern is only applied to these files; everywhere
    else, ``exclude = [...]`` is out of scope (likely publish-config or
    similar non-test concern)."""
    p = rel_path.as_posix()
    return p in KNOWN_CONFIG_FILES


def scan_file(file_path: Path, repo_root: Path) -> list[Hit]:
    """Scan a single file for unmarked suppressions. Returns a list of
    ``Hit`` entries, one per offending line."""
    rel = file_path.relative_to(repo_root)
    rel_posix = rel.as_posix()
    if rel_posix in HARD_SKIP_PATHS:
        return []

    suffix = file_path.suffix.lower()
    try:
        text = file_path.read_text()
    except (OSError, UnicodeDecodeError):
        return []
    lines = text.splitlines()

    hits: list[Hit] = []
    for pattern in PATTERNS:
        if pattern.suffixes and suffix not in pattern.suffixes:
            continue
        # The `config-array` pattern is filename-allowlisted: a TOML/YAML
        # file outside KNOWN_CONFIG_FILES may legitimately have an
        # `exclude = [...]` line (publish config, cargo-deny exemptions,
        # release-please config, etc.). Skip those.
        if pattern.name == "config-array" and not _is_known_config(rel):
            continue

        for line_no, line in enumerate(lines, start=1):
            if not pattern.regex.search(line):
                continue
            window = _line_window(lines, line_no)
            if MARKER_RE.search(window):
                continue
            hits.append(
                Hit(
                    file=rel,
                    line_no=line_no,
                    line_text=line,
                    pattern_name=pattern.name,
                    pattern_description=pattern.description,
                )
            )
    return hits


def iter_files(repo_root: Path) -> list[Path]:
    """Expand ``SCAN_GLOBS`` + ``KNOWN_CONFIG_FILES`` against the repo
    root, returning a deduplicated, sorted list of absolute paths that
    exist on disk."""
    found: set[Path] = set()
    for glob in SCAN_GLOBS:
        for path in repo_root.glob(glob):
            if path.is_file():
                found.add(path.resolve())
    for known in KNOWN_CONFIG_FILES:
        candidate = repo_root / known
        if candidate.is_file():
            found.add(candidate.resolve())
    return sorted(found)


def lint(repo_root: Path) -> int:
    files = iter_files(repo_root)
    all_hits: list[Hit] = []
    for file_path in files:
        all_hits.extend(scan_file(file_path, repo_root))

    if all_hits:
        print(
            "tracked-lint: unmarked test/coverage suppressions found.\n"
            "Each suppression below lacks an adjacent "
            "`tracked: <repo>#<n>` or `adr: <path>` reference.\n"
            "See ~/.claude/rules/exclusions.md for the rule.\n",
            file=sys.stderr,
        )
        for hit in all_hits:
            print(
                f"::error file={hit.file},line={hit.line_no}::"
                f"tracked-lint[{hit.pattern_name}]: "
                f"{hit.pattern_description} lacks tracked/adr reference",
                file=sys.stderr,
            )
            print(
                f"\n  {hit.file}:{hit.line_no}\n"
                f"    pattern: {hit.pattern_name} ({hit.pattern_description})\n"
                f"    line:    {hit.line_text.strip()}\n"
                f"    fix:     add a comment within {PRE_WINDOW} lines above "
                f"or {POST_WINDOW} line below in the shape\n"
                f"               tracked: <repo>#<n> "
                "—"
                f" <one-line reason>\n"
                f"             OR\n"
                f"               adr: <relative-path-to-adr> "
                "—"
                f" <one-line reason>\n"
                f"             where <repo> is a Breezy Bays Labs repo name "
                f"(dry-rs, mokumo, crap4rs, scrap-rs, ops, ...).",
                file=sys.stderr,
            )
        plural = "" if len(all_hits) == 1 else "s"
        print(
            f"\ntracked-lint: {len(all_hits)} unmarked suppression{plural} "
            f"across {len({h.file for h in all_hits})} file(s); see above.",
            file=sys.stderr,
        )
        return 1

    print(
        f"tracked-lint: ok ({len(files)} files scanned; "
        f"all test/coverage suppressions carry a tracked/adr reference, "
        f"or none exist)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(lint(Path(__file__).resolve().parent.parent))
