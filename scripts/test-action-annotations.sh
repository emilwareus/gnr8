#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
python3 - "$tmp/report.json" <<'PY'
import json, sys
changes = []
for kind, gating in [('breaking', True), ('breaking', False), ('additive', False), ('doc_only', False)]:
    changes.append(dict(kind=kind, gating=gating, code='request.property.required.added',
                        message='hostile\n%::,\rmessage', file='main.go', line=25, span=dict(end_line=25)))
changes.append(dict(kind='breaking', gating=True, code='operation.removed', message='removed', file=None, line=None, span=None))
changes.extend([changes[0].copy() for _ in range(60)])
with open(sys.argv[1], 'w') as out:
    json.dump(dict(schema_version=1, changes=changes), out)
PY
python3 "$repo_root/scripts/emit-action-annotations.py" "$tmp/report.json" examples/bookstore test-artifact > "$tmp/commands"
grep -Fx '::error file=examples/bookstore/main.go,line=25,endLine=25,title=gnr8%3A request.property.required.added::hostile%0A%25::,%0Dmessage' "$tmp/commands" >/dev/null
grep -F '::warning file=examples/bookstore/main.go' "$tmp/commands" >/dev/null
grep -F '::notice file=examples/bookstore/main.go' "$tmp/commands" >/dev/null
test "$(grep -c '^::.* file=' "$tmp/commands")" -eq 50
grep -Fx '::notice::gnr8: 14 further findings not annotated (1 unanchorable); see the job summary and the "test-artifact" artifact.' "$tmp/commands" >/dev/null
test "$(wc -l < "$tmp/commands")" -eq 51
! grep -F 'operation.removed' "$tmp/commands"
printf '{"schema_version":2,"changes":[]}' > "$tmp/report.json"
if python3 "$repo_root/scripts/emit-action-annotations.py" "$tmp/report.json" . report > "$tmp/commands" 2> "$tmp/error"; then exit 1; fi
grep -F 'gnr8 action: cannot emit API change annotations' "$tmp/error" >/dev/null
test ! -s "$tmp/commands"
echo 'action annotation tests: OK (2 cases)'
