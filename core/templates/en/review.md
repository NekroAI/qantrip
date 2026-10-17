---
name: review
aliases: [r]
description: Code review
category: Review & test
params:
  focus:
    type: multi
    positional: true
    description: What to focus on (multiple allowed)
    choices:
      - { value: correctness, label: Correctness, aliases: [bug] }
      - { value: security, label: Security, aliases: [sec] }
      - { value: performance, label: Performance, aliases: [perf] }
      - { value: maintainability, label: Maintainability, aliases: [maint] }
      - { value: tests, label: Test coverage }
  scope:
    type: enum
    description: Which changes to review
    default: changes
    choices:
      - { value: changes, label: All uncommitted changes }
      - { value: staged, label: Staged changes }
      - { value: branch, label: Current branch }
      - { value: last, label: Last commit }
  strict:
    type: bool
    short: s
    description: "Strict mode: report every doubt"
---
Review {% include "_scope" %}.

Understand the intent of the change before looking for problems.
{% if focus %}
Focus on: {% for f in focus %}{{ {"correctness": "correctness and edge cases", "security": "security (permissions, input validation, secrets, injection)", "performance": "performance and resource usage", "maintainability": "readability and maintainability", "tests": "whether the tests cover the changed behavior"}[f] }}{% if not loop.last %}; {% endif %}{% endfor %}.
{% else %}
Pay attention to correctness, edge cases, security and maintainability.
{% endif %}

Output:
1. List issues by severity, each with file and line, the cause, and a suggested fix.
2. Separate confirmed problems from doubts that need verification.
{% if strict %}
3. Strict mode: include anything uncertain and give a confidence level; don't leave things out for brevity.
{% else %}
3. Report only issues that are genuinely worth changing; skip style preferences.
{% endif %}

Do not modify the code.
{% include "_extra" %}
