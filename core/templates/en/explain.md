---
name: explain
aliases: [ex]
description: Explain code
category: Understand & plan
params:
  level:
    type: enum
    positional: true
    description: Level of detail
    default: detailed
    choices:
      - { value: brief, label: One-line summary }
      - { value: detailed, label: Detailed }
      - { value: beginner, label: For beginners }
  diagram:
    type: bool
    short: d
    description: Include a Mermaid diagram
---
Explain {% if extra %}{{ extra }}{% else %}this code{% endif %}.

{% if level == "brief" %}
In a few sentences, say what it does and why it is done this way.
{% elif level == "beginner" %}
Assume I'm new to this codebase and its technologies: start with the problem it solves, then walk through the key steps in execution order, explaining terms as they come up.
{% else %}
Cover: the problem it solves; the main flow and data flow; key design decisions and why they were made; anything error-prone or non-obvious.
{% endif %}
{% if diagram %}
Include a Mermaid flowchart of the main flow.
{% endif %}
Explain only; don't modify the code.
