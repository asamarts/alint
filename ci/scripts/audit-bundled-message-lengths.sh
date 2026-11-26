#!/usr/bin/env bash
# T1.2 (v0.9.20): audit bundled-rule message lengths.
#
# Walks every `crates/alint-dsl/rulesets/**/*.yml`, extracts each
# rule's `message:` field, and reports any whose effective text is
# too long for a single wrapped line at MSG_INDENT (14 cols) on an
# 80-col terminal.
#
# Budget: 80 cols total - 14 cols indent = 66 chars of message text
# per wrapped line. Rules above the budget should either be tightened
# or deliberately accept the wrap (set in the per-rule comment).
#
# Exit 0 always (informational by default). Pass --fail-over <N> to
# fail with non-zero when any message exceeds <N> chars (CI gate).
#
# Status: manual maintainer helper — intentionally NOT wired into
# ci.yml or preflight.sh. Run it (optionally with --fail-over) when
# adding or editing bundled-ruleset `message:` strings; auto-gating
# would false-fail on messages that deliberately accept a wrap.

set -euo pipefail

BUDGET=66    # default soft target
HARD_FAIL=0  # 0 = info only; if --fail-over N provided, set to N

while [[ $# -gt 0 ]]; do
    case "$1" in
        --budget)     BUDGET="$2"; shift 2 ;;
        --fail-over)  HARD_FAIL="$2"; shift 2 ;;
        --help|-h)
            sed -n '2,16p' "$0" | sed 's/^# \?//'
            exit 0
            ;;
        *) echo "Unknown arg: $1" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RULESETS_DIR="$REPO_ROOT/crates/alint-dsl/rulesets"

cd "$REPO_ROOT"

python3 - "$RULESETS_DIR" "$BUDGET" "$HARD_FAIL" << 'PYEOF'
import os, sys, glob

rulesets_dir = sys.argv[1]
budget = int(sys.argv[2])
hard_fail = int(sys.argv[3])

# Walk every YAML and extract (file, rule_id, message_text).
# Naive YAML walk — alint-dsl uses block scalars (`>-` and `|-`) for
# messages. We need to handle both single-line and folded-block
# scalars. For audit-purposes we collapse folded blocks to one line
# (which is how they render at runtime).

def extract_rules(yaml_text, file_path):
    """Yield (rule_id, message_text, line_no) tuples."""
    lines = yaml_text.splitlines()
    i = 0
    current_id = None
    while i < len(lines):
        line = lines[i]
        stripped = line.lstrip()
        if stripped.startswith('- id:'):
            # New rule
            current_id = stripped.split(':', 1)[1].strip()
        elif stripped.startswith('message:'):
            # Capture message — could be inline, plain block (`message: hello`),
            # double-quoted, single-quoted, or folded (`>-` / `|-` / `>` / `|`).
            after = stripped.split(':', 1)[1].strip()
            if after.startswith('>') or after.startswith('|'):
                # Block scalar — read continuation lines that are deeper indented
                base_indent = len(line) - len(stripped)
                msg_lines = []
                j = i + 1
                while j < len(lines):
                    next_line = lines[j]
                    if next_line.strip() == '':
                        msg_lines.append('')
                        j += 1
                        continue
                    next_indent = len(next_line) - len(next_line.lstrip())
                    if next_indent <= base_indent:
                        break
                    msg_lines.append(next_line[base_indent + 2:])
                    j += 1
                # Folded (>) collapses on whitespace; literal (|) preserves.
                # For runtime length, folded mode collapses linebreaks to spaces.
                if after[0] == '>':
                    text = ' '.join(l.strip() for l in msg_lines if l.strip())
                else:
                    text = '\n'.join(msg_lines).rstrip()
                yield (current_id, text, i + 1)
                i = j
                continue
            else:
                # Inline message — strip surrounding quotes if present
                text = after
                if (text.startswith('"') and text.endswith('"')) or \
                   (text.startswith("'") and text.endswith("'")):
                    text = text[1:-1]
                yield (current_id, text, i + 1)
        i += 1

over_budget = []
total_rules = 0

for yaml_path in sorted(glob.glob(os.path.join(rulesets_dir, '**', '*.yml'), recursive=True)):
    rel = os.path.relpath(yaml_path, rulesets_dir)
    with open(yaml_path) as f:
        text = f.read()
    for rule_id, message, line_no in extract_rules(text, yaml_path):
        if message is None or rule_id is None:
            continue
        total_rules += 1
        # For multi-line messages, check the longest line
        max_line_len = max(len(line) for line in message.split('\n')) if message else 0
        if max_line_len > budget:
            over_budget.append((rel, line_no, rule_id, max_line_len, message))

over_budget.sort(key=lambda x: -x[3])

print(f"Bundled-rule message-length audit")
print(f"  rulesets dir: {rulesets_dir}")
print(f"  budget: {budget} chars per line (= 80-col terminal - 14-col MSG_INDENT)")
print(f"  rules scanned: {total_rules}")
print(f"  over budget: {len(over_budget)}")
print()

if over_budget:
    print(f"{'Length':>6}  {'Rule ID':<48}  Source")
    print(f"{'-'*6}  {'-'*48}  {'-'*40}")
    for rel, line_no, rule_id, length, message in over_budget:
        print(f"{length:>6}  {rule_id:<48}  {rel}:{line_no}")
    print()
    print("Top 5 messages (full text):")
    for rel, line_no, rule_id, length, message in over_budget[:5]:
        first_line = message.split('\n')[0]
        print(f"  [{length}] {rule_id}")
        print(f"    {first_line[:100]}{'...' if len(first_line) > 100 else ''}")

if hard_fail > 0:
    fails = [r for r in over_budget if r[3] > hard_fail]
    if fails:
        print(f"\nFAIL: {len(fails)} message(s) exceed {hard_fail} chars (hard limit).")
        sys.exit(1)
    print(f"\nOK: no message exceeds the hard limit of {hard_fail} chars.")

PYEOF
