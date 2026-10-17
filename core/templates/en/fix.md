---
name: fix
description: "Fix a bug: find the root cause, change as little as possible"
category: Write & fix
params:
  mode:
    type: enum
    description: Execution mode
    default: apply
    choices:
      - { value: read, label: Root cause only }
      - { value: suggest, label: Propose a fix }
      - { value: apply, label: Fix it }
  test:
    type: bool
    short: t
    description: Write a failing test first
---
Fix the problem described below.
{% if extra %}

Problem: {{ extra }}
{% endif %}

Requirements:
1. Find the root cause and explain why it happens; don't just treat the symptom.
{% if test %}
2. Before fixing, write a test that reproduces the problem and confirm it fails; confirm it passes after the fix.
{% else %}
2. After fixing, explain how to verify the problem is gone.
{% endif %}
3. Check whether the same root cause exists elsewhere.

{% include "_mode" %}
