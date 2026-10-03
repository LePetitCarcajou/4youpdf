"""PreToolUse hook of the `testeur` subagent (.claude/agents/testeur.md).

The tester may write under target/agents/ and to docs/sessions/*-tests.md,
nowhere else: it proves defects, the main session fixes them. Exit code 2
blocks the tool call and shows the message on stderr to the subagent.
"""

import json
import os
import sys
from pathlib import Path


def refuse(message: str) -> int:
    print(f"garde du testeur : {message}", file=sys.stderr)
    return 2


def main() -> int:
    try:
        data = json.load(sys.stdin)
    except ValueError:
        return refuse("entrée du hook illisible, écriture refusée")
    tool_input = data.get("tool_input") or {}
    target = tool_input.get("file_path") or tool_input.get("notebook_path")
    if not target:
        return refuse("aucun chemin dans la demande, écriture refusée")

    root = Path(
        os.environ.get("CLAUDE_PROJECT_DIR") or data.get("cwd") or os.getcwd()
    ).resolve()
    path = Path(target)
    if not path.is_absolute():
        path = root / path
    path = path.resolve()

    try:
        parts = path.relative_to(root).parts
    except ValueError:
        parts = ()
    if parts[:2] == ("target", "agents") and len(parts) > 2:
        return 0
    if (
        len(parts) == 3
        and parts[:2] == ("docs", "sessions")
        and parts[2].endswith("-tests.md")
    ):
        return 0
    return refuse(
        f"{target} est hors de target/agents/ et de docs/sessions/*-tests.md ; "
        "le testeur ne modifie ni le code, ni les tests, ni la documentation "
        "du dépôt : décris le test à ajouter dans ton compte rendu"
    )


if __name__ == "__main__":
    sys.exit(main())
