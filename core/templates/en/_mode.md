---
name: _mode
description: "Shared snippet: execution mode (read-only / suggest / apply)"
---
{% if mode == "read" %}
Analyze only. Do not modify any files.
{% elif mode == "suggest" %}
Propose concrete changes (including code), but do not edit any files until I confirm.
{% else %}
You may edit the code directly. Keep the change minimal: touch only what this task needs, with no unrelated refactoring, renaming or reformatting.
{% endif %}
