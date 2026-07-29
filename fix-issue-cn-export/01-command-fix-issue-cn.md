Read the file `skills/04-review/fix-review-issue.skill` in the current project and follow its workflow exactly.

Language override (highest priority for this command):
- All narrative text addressed to the user — explanations, prompts, questions, verdict/impact panels, summaries, and the `--- Current position ---` status line — MUST be written in Simplified Chinese, regardless of the user's input language.
- This override only changes how you talk to the user. It does NOT change what gets written to disk: code, comments, doc comments, commit messages, log/error literals, file paths, issue IDs, and quoted source text stay in their original language (English where the project requires it). The English-only-on-disk rule still applies in full.
- Template section headings may be translated to Chinese to match the narrative.

Arguments: $ARGUMENTS

Argument parsing rules (apply before Pre-flight P0):
- If $ARGUMENTS contains "/" → treat it as the ISSUE_DIR path
- If $ARGUMENTS looks like an issue ID (e.g. "c1-012", "g3-016") → search `tasks/` for a matching file
- If $ARGUMENTS contains both (e.g. "tasks/review-p3/ c1-012") → first token is ISSUE_DIR, second is the issue ID
- If $ARGUMENTS is empty → follow the skill's auto-detection logic (find tasks/review-*/ directories)
