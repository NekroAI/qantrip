---
name: debug
description: "Deep debugging: locate and verify the cause"
category: Write & fix
params:
  mode:
    type: enum
    description: Execution mode
    default: read
    choices:
      - { value: read, label: Locate only }
      - { value: suggest, label: Locate and propose a fix }
      - { value: apply, label: Locate and fix }
  log:
    type: bool
    short: l
    description: Allow temporary logging
---
Help me debug this problem.
{% if extra %}

Symptom: {{ extra }}
{% endif %}

Work through it step by step:
1. Restate the symptom and list what is known; keep facts and guesses separate.
2. Propose up to three possible causes, most likely first, with a way to verify each.
3. Verify them one at a time. Let evidence drive each step; don't change code on a hunch.
{% if log %}
4. You may add temporary logging or debug output to gather evidence; remove all of it when done.
{% endif %}

Finish with the root cause and the evidence for it.
{% include "_mode" %}
