#!/usr/bin/env python3
"""Bot-context drift lint — mechanical enforcement of AGENTS.md ↔ Cargo.toml
consistency.

The ``AGENTS.md`` "For automated code reviewers" section grounds AI bots
(CodeRabbit, gemini-code-assist) in the project's load-bearing rules
(per-crate allowed deps, AST-purity scope, locked wire shapes). The
per-crate dep table under ``## Architecture`` is the **authoritative**
source the bot-context section references. If that table drifts from
the actual ``Cargo.toml`` files, bots ground their suggestions in stale
data and any "Allowed deps" claim becomes unenforceable.

This lint catches drift in **both directions**:

1. **Missing-in-table** — a crate's ``Cargo.toml`` ``[dependencies]``
   carries a dep that the AGENTS.md row doesn't list. (Bot would tell
   a reviewer the dep isn't permitted; reality says it is.)
2. **Extra-in-table** — AGENTS.md lists a dep the crate's
   ``Cargo.toml`` doesn't carry. (Aspirational entries that never
   landed; bot grants permissions the codebase never asked for.)

Scope: **internal consistency only** (option (c) of dry-rs#26's
Discovery section). Cross-repo verification against the source ADRs in
the private ``ops`` vault is deferred — that needs a deploy key + cross-
repo CI access, out of scope at v0.1.

Same mechanical-enforcement pattern as ``scripts/tracked-lint.py``:
single source of truth in this script; ``.github/workflows/bot-context-
drift.yml`` and ``lefthook.yml`` both invoke it. Exit-code semantics:
``0`` on no drift, ``1`` on any drift found, with ``::error::``
annotations for each finding so GitHub Actions surfaces them inline.

---

**AGENTS.md table format** (under ``## Architecture``):

.. code-block:: markdown

   | Crate | Purpose | Allowed deps |
   |-------|---------|--------------|
   | `dry-core` | <purpose> | `serde` (derive), `serde_json`, ...
   | `dry4rs`   | <purpose> | `dry-core`, `syn`, `proc-macro2` (with `span-locations` feature), ...

The parser extracts each row's "Allowed deps" cell, splits on commas,
and pulls the first backtick-quoted token from each comma-separated
fragment as the dep name. Trailing annotations like ``(derive)``,
``(with `span-locations` feature)``, ``*or* `oxc_parser```` are
ignored — they're noise after the dep name. This handles the nested-
backtick case (`` `xxhash-rust` (with `xxh3` feature) ``) without
naively iterating every backtick pair on the line.

**Crate matching**: each AGENTS.md row's first column (the crate name
in backticks) maps to ``crates/<name>/Cargo.toml``. Rows where the
crate directory doesn't exist on disk are **skipped silently** — this
is the v0.6+ ``dry4ts`` row case: the table documents the future
adapter, but the crate hasn't joined the workspace yet. The lint
activates structurally when the crate dir lands.

**Cargo.toml parsing**: ``tomllib`` from the Python 3.11+ stdlib. Only
``[dependencies]`` is scanned; ``[dev-dependencies]`` is intentionally
excluded — AGENTS.md's table tracks runtime/build deps only, since
that's where the layering invariant matters (a dev-dep can't violate
the hexagonal direction at compile time).

**Self-reference defense**: this script's own path is in
``HARD_SKIP_PATHS`` — defense-in-depth against the lint catching its
own source. The path-allowlist (only ``AGENTS.md`` and
``crates/*/Cargo.toml`` are read) already excludes ``scripts/`` so
this is belt-and-suspenders.
"""

from __future__ import annotations

import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

# The AGENTS.md file at repo root carries the authoritative dep table.
AGENTS_MD = "AGENTS.md"

# Section header that introduces the per-crate dep table. The table is
# the FIRST markdown table appearing after this header. A future
# section before ``## Architecture`` could theoretically include a
# pipe-delimited markdown table; anchoring to the section header avoids
# that false-positive. Anchored to a line-leading ``## `` so subsection
# variants (``### Architecture``) don't catch.
ARCHITECTURE_HEADER_RE = re.compile(r"^##\s+Architecture\s*$")

# A markdown-table row line: starts with ``|`` after optional whitespace,
# has at least two more ``|`` separators. The header + separator + data
# rows all match; the separator row is filtered out by checking for
# at least one alphanumeric content char.
TABLE_ROW_RE = re.compile(r"^\s*\|.+\|.*$")

# Repo-relative path to this script — added to HARD_SKIP_PATHS as
# defense-in-depth even though the scan paths already don't include it.
HARD_SKIP_PATHS: frozenset[str] = frozenset(
    {
        "scripts/bot-context-drift.py",
    }
)

# ---------------------------------------------------------------------------
# Data structures
# ---------------------------------------------------------------------------


@dataclass
class CrateRow:
    """Parsed row from the AGENTS.md per-crate dep table.

    ``allowed_deps`` is a set of dep names normalized to lower-case-
    alphanumeric-plus-hyphens (the crate-name convention on crates.io).
    ``raw_text`` keeps the un-normalized table cell for diagnostic
    output when drift is detected.
    """

    crate: str
    allowed_deps: frozenset[str]
    line_no: int
    raw_text: str


@dataclass
class Finding:
    """One drift finding emitted by the lint."""

    kind: str  # "missing-in-table" or "extra-in-table"
    crate: str
    dep: str
    detail: str
    file: str  # File the finding is anchored to (AGENTS.md or Cargo.toml).
    line_no: int  # 1-indexed.


# ---------------------------------------------------------------------------
# AGENTS.md table parser
# ---------------------------------------------------------------------------


def _extract_dep_name(fragment: str) -> str | None:
    """Pull the first backtick-quoted token out of a comma-separated
    table-cell fragment. Returns ``None`` if the fragment has no
    backticks (e.g. empty cell, or annotation-only like ``*or* foo``).

    Handles nested-backtick case: ```xxhash-rust` (with `xxh3` feature)``
    returns ``xxhash-rust`` and discards the rest. We're intentionally
    NOT scanning every backtick pair on the fragment — only the FIRST,
    because the dep name is by convention always the first
    backtick-quoted token.

    The fragment may be free-form (``*or* `oxc_parser```), conditional
    (``(with `span-locations` feature)``), or annotated (``(derive)``).
    All of these are "noise after the dep name" and tolerated.
    """
    # Match the FIRST `<dep-name>` token. Dep name is conventional crate-
    # name shape: lowercase alphanumeric + hyphens + underscores.
    m = re.search(r"`([A-Za-z0-9][A-Za-z0-9_\-]*)`", fragment)
    if not m:
        return None
    return m.group(1).lower()


def parse_agents_table(agents_md_path: Path) -> tuple[list[CrateRow], int]:
    """Locate the per-crate dep table under ``## Architecture`` in
    AGENTS.md and parse each data row.

    Returns ``(rows, table_start_line)``. ``table_start_line`` is the
    1-indexed line where the table's header row sits — used to anchor
    error annotations to a real line in AGENTS.md so GitHub Actions can
    link them.

    Raises ``ValueError`` if the section header isn't found or no table
    is detected — drift detection is structurally meaningless if the
    table itself moved or vanished, and silent failure would let drift
    slip in.
    """
    text = agents_md_path.read_text()
    lines = text.splitlines()

    arch_start = -1
    for idx, line in enumerate(lines):
        if ARCHITECTURE_HEADER_RE.match(line):
            arch_start = idx
            break
    if arch_start < 0:
        raise ValueError(
            f"{agents_md_path}: '## Architecture' section header not found"
        )

    # The table is the first contiguous block of pipe-delimited lines
    # following the header. Skip the separator row (``|---|---|``); any
    # row whose content cells are all dashes-plus-whitespace.
    table_start = -1
    in_table = False
    rows: list[CrateRow] = []
    for idx in range(arch_start + 1, len(lines)):
        line = lines[idx]
        if TABLE_ROW_RE.match(line):
            if not in_table:
                table_start = idx + 1  # 1-indexed
                in_table = True
            # Skip the header row + separator row.
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            # Separator row: all cells are made of ``-`` + whitespace +
            # optional colons (alignment markers).
            if all(re.fullmatch(r":?-+:?", c) for c in cells if c):
                continue
            # Header row: starts with ``Crate`` (case-insensitive). Skip.
            if cells and cells[0].lower() == "crate":
                continue
            if len(cells) < 3:
                # Malformed row — record nothing, but don't fail. A
                # subsequent table-shape regression would surface via a
                # CI dry-run; this script's job is drift detection, not
                # table-shape validation.
                continue
            # First cell = crate name in backticks.
            crate_match = re.search(r"`([^`]+)`", cells[0])
            if not crate_match:
                continue
            crate = crate_match.group(1)
            # Third cell = comma-separated allowed deps.
            deps_cell = cells[2]
            deps: set[str] = set()
            # Split on commas at the TOP level. Backticks don't nest in
            # our table, but they DO appear inside parens after a comma-
            # separated dep — e.g. ```serde` (derive), `serde_json```.
            # A simple comma split is correct because the noise lives
            # AFTER the first backtick-quoted token, not BEFORE it.
            for fragment in deps_cell.split(","):
                dep = _extract_dep_name(fragment.strip())
                if dep is not None:
                    deps.add(dep)
            rows.append(
                CrateRow(
                    crate=crate,
                    allowed_deps=frozenset(deps),
                    line_no=idx + 1,
                    raw_text=line.strip(),
                )
            )
        elif in_table:
            # Table ended (blank line or non-pipe line).
            break

    if not rows:
        raise ValueError(
            f"{agents_md_path}: no per-crate dep table rows found under "
            f"'## Architecture'"
        )

    return rows, table_start


# ---------------------------------------------------------------------------
# Cargo.toml parser
# ---------------------------------------------------------------------------


def parse_cargo_deps(cargo_toml: Path) -> set[str]:
    """Return the set of dep names from the crate's ``[dependencies]``
    table. ``[dev-dependencies]`` and ``[build-dependencies]`` are
    intentionally NOT included — AGENTS.md's table tracks runtime
    deps because the hexagonal-layering invariant only applies at
    compile time. Dev-deps can use anything (a dev-dep on ``syn``
    inside ``dry-core/[dev-dependencies]`` is fine, for instance).

    Dep names are lowercased to match the normalization in
    ``_extract_dep_name``.
    """
    with cargo_toml.open("rb") as f:
        data = tomllib.load(f)
    deps = data.get("dependencies", {})
    return {name.lower() for name in deps.keys()}


# ---------------------------------------------------------------------------
# Drift detection
# ---------------------------------------------------------------------------


def detect_drift(
    rows: list[CrateRow], repo_root: Path
) -> list[Finding]:
    """For each row whose crate dir exists on disk, compare the
    AGENTS.md-allowed set against the actual ``[dependencies]`` set.
    Bidirectional check — both ``missing-in-table`` (Cargo.toml has a
    dep AGENTS.md doesn't list) and ``extra-in-table`` (AGENTS.md
    lists a dep Cargo.toml doesn't have) are findings.

    Rows whose crate dir doesn't exist (e.g. ``dry4ts`` before v0.6+)
    are skipped silently — see module docstring.
    """
    findings: list[Finding] = []
    for row in rows:
        crate_dir = repo_root / "crates" / row.crate
        cargo_toml = crate_dir / "Cargo.toml"
        if not cargo_toml.is_file():
            continue
        actual = parse_cargo_deps(cargo_toml)
        allowed = set(row.allowed_deps)

        for missing in sorted(actual - allowed):
            findings.append(
                Finding(
                    kind="missing-in-table",
                    crate=row.crate,
                    dep=missing,
                    detail=(
                        f"crate `{row.crate}` depends on `{missing}` in "
                        f"Cargo.toml, but AGENTS.md's per-crate dep table "
                        f"does not list it as an allowed dep"
                    ),
                    file="AGENTS.md",
                    line_no=row.line_no,
                )
            )
        for extra in sorted(allowed - actual):
            findings.append(
                Finding(
                    kind="extra-in-table",
                    crate=row.crate,
                    dep=extra,
                    detail=(
                        f"AGENTS.md lists `{extra}` as an allowed dep for "
                        f"`{row.crate}`, but the dep is not present in "
                        f"`crates/{row.crate}/Cargo.toml [dependencies]`"
                    ),
                    file="AGENTS.md",
                    line_no=row.line_no,
                )
            )

    return findings


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def lint(repo_root: Path) -> int:
    """Return ``0`` on no drift, ``1`` on any drift found."""
    agents_md = repo_root / AGENTS_MD
    if not agents_md.is_file():
        print(
            f"bot-context-drift: {AGENTS_MD} not found at repo root "
            f"({agents_md}); nothing to check.",
            file=sys.stderr,
        )
        return 1

    # Self-defense: refuse to scan if this script is somehow on the
    # filesystem path we'd walk. We don't actually walk anything; the
    # path allowlist is static (AGENTS.md + crates/*/Cargo.toml). The
    # HARD_SKIP_PATHS check is here as belt-and-suspenders mirror of
    # tracked-lint.py's pattern.
    script_rel = Path("scripts") / "bot-context-drift.py"
    if script_rel.as_posix() not in HARD_SKIP_PATHS:
        # This is structurally impossible (the constant above lists it),
        # but the check documents the invariant.
        print(
            f"bot-context-drift: internal invariant broken — "
            f"{script_rel.as_posix()} missing from HARD_SKIP_PATHS",
            file=sys.stderr,
        )
        return 1

    try:
        rows, table_start = parse_agents_table(agents_md)
    except ValueError as exc:
        print(
            f"::error file={AGENTS_MD}::bot-context-drift: "
            f"AGENTS.md table parse failed: {exc}",
            file=sys.stderr,
        )
        print(
            f"bot-context-drift: cannot extract per-crate dep table from "
            f"{AGENTS_MD}.\nThe lint is structurally meaningless without "
            f"a parseable table; see the script docstring for the "
            f"expected format.",
            file=sys.stderr,
        )
        return 1

    findings = detect_drift(rows, repo_root)

    if findings:
        print(
            "bot-context-drift: AGENTS.md per-crate dep table drifted "
            "from Cargo.toml.\n"
            "Each finding below is one of:\n"
            "  - missing-in-table: Cargo.toml has a dep AGENTS.md "
            "doesn't list (table is stale / dep added unilaterally)\n"
            "  - extra-in-table:  AGENTS.md lists a dep that's not in "
            "Cargo.toml (aspirational entry / dep removed without "
            "table update)\n"
            "Fix: either add/remove the dep in AGENTS.md's table, or "
            "update Cargo.toml to match the table (per the layering "
            "ADR's allowed-deps invariant).\n",
            file=sys.stderr,
        )
        for finding in findings:
            print(
                f"::error file={finding.file},line={finding.line_no}::"
                f"bot-context-drift[{finding.kind}]: {finding.detail}",
                file=sys.stderr,
            )
            print(
                f"\n  {finding.file}:{finding.line_no} "
                f"[{finding.kind}]\n"
                f"    crate: {finding.crate}\n"
                f"    dep:   {finding.dep}\n"
                f"    detail: {finding.detail}",
                file=sys.stderr,
            )
        plural = "" if len(findings) == 1 else "s"
        print(
            f"\nbot-context-drift: {len(findings)} drift finding{plural} "
            f"across {len({f.crate for f in findings})} crate(s); see "
            f"above.",
            file=sys.stderr,
        )
        return 1

    crate_count = sum(
        1
        for row in rows
        if (repo_root / "crates" / row.crate / "Cargo.toml").is_file()
    )
    skipped = len(rows) - crate_count
    skipped_note = f" ({skipped} row(s) skipped — crate dir absent)" if skipped else ""
    print(
        f"bot-context-drift: ok ({crate_count} crate(s) checked "
        f"against AGENTS.md table at line {table_start}){skipped_note}."
    )
    return 0


if __name__ == "__main__":
    sys.exit(lint(Path(__file__).resolve().parent.parent))
