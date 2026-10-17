---
name: commit
aliases: [ci]
description: Write a commit message
category: Ship
params:
  style:
    type: enum
    positional: true
    description: Format
    default: conventional
    choices:
      - { value: conventional, label: Conventional Commits }
      - { value: plain, label: Plain }
  lang:
    type: enum
    description: Language
    default: en
    choices:
      - { value: en, label: English }
      - { value: zh, label: Chinese }
  run:
    type: bool
    description: Commit when done
---
Write a commit message in {{ "Chinese" if lang == "zh" else "English" }} for the staged changes (git diff --cached).

{% if style == "conventional" %}
Follow Conventional Commits: `type(scope): subject`, with type one of feat, fix, refactor, docs, test, chore, perf.
{% endif %}
- Keep the subject under 72 characters and say what changed.
- Use the body to explain why, and anything to watch out for; simple changes can skip the body.
- Match the style of existing commits in git log.

{% if run %}
Run git commit once it looks right.
{% else %}
Output the message only; don't commit.
{% endif %}
{% include "_extra" %}
