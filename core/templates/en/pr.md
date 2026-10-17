---
name: pr
description: Write a pull request description
category: Ship
params:
  sections:
    type: multi
    description: Sections to include
    default: [summary, changes, testing]
    choices:
      - { value: summary, label: Summary }
      - { value: changes, label: Changes }
      - { value: testing, label: Testing }
      - { value: risks, label: Risks and impact }
      - { value: followup, label: Follow-up work }
  lang:
    type: enum
    description: Language
    default: en
    choices:
      - { value: en, label: English }
      - { value: zh, label: Chinese }
  create:
    type: bool
    description: Create the PR with gh
---
Write a pull request description in {{ "Chinese" if lang == "zh" else "English" }} for all changes on the current branch relative to the main branch, with these sections:
{% for s in sections %}
- {{ {"summary": "Summary: what problem this solves and why it's needed", "changes": "Changes: the main changes, grouped by module", "testing": "Testing: how it was verified and which cases are covered", "risks": "Risks and impact: compatibility, migrations, performance", "followup": "Follow-up: what was deliberately left out of this PR"}[s] }}
{% endfor %}

Only include what can be confirmed from the changes. {% if create %}Create the PR with gh pr create when done.{% else %}Output the description only; don't create the PR.{% endif %}
{% include "_extra" %}
