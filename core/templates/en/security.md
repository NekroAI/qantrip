---
name: security
aliases: [sec]
description: Security review
category: Review & test
params:
  focus:
    type: multi
    positional: true
    description: What to check
    choices:
      - { value: auth, label: Authentication and authorization }
      - { value: input, label: Input validation }
      - { value: injection, label: Injection }
      - { value: secrets, label: Secrets }
      - { value: deps, label: Dependencies }
  scope:
    type: enum
    description: Which changes to review
    default: changes
    choices:
      - { value: changes, label: All uncommitted changes }
      - { value: staged, label: Staged changes }
      - { value: branch, label: Current branch }
      - { value: last, label: Last commit }
---
Do a security review of {% include "_scope" %}{% if focus %}, focusing on {% for f in focus %}{{ {"auth": "authentication and access control", "input": "input validation", "injection": "injection (SQL, command, template, path)", "secrets": "leaked keys and sensitive data", "deps": "known vulnerabilities in dependencies"}[f] }}{% if not loop.last %}, {% endif %}{% endfor %}{% endif %}.

For each finding:
1. Explain how an attacker would exploit it (preconditions and impact), without giving a ready-to-use payload.
2. Rate severity (high / medium / low) and your confidence.
3. Suggest a fix.

Separate confirmed exploitable issues from ones that need further verification. Do not modify the code.
{% include "_extra" %}
