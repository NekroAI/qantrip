---
name: docs
description: Write documentation
category: Ship
params:
  kind:
    type: enum
    positional: true
    description: Kind of document
    default: usage
    choices:
      - { value: api, label: API reference }
      - { value: readme, label: README }
      - { value: usage, label: Usage guide }
      - { value: comments, label: Code comments }
      - { value: changelog, label: Changelog }
  audience:
    type: enum
    description: Audience
    default: developer
    choices:
      - { value: user, label: End users }
      - { value: developer, label: Developers }
---
Write {{ {"api": "an API reference", "readme": "a README", "usage": "a usage guide", "comments": "code comments", "changelog": "a changelog"}[kind] }} for {% if extra %}{{ extra }}{% else %}this code{% endif %}, aimed at {{ "end users" if audience == "user" else "developers" }}.

Requirements:
1. Only write what can be confirmed from the code; flag anything uncertain and ask me.
2. Start with the most common usage example, then the details.
{% if kind == "comments" %}
3. Comments should explain why, not restate what the code does; match the project's existing comment style and density.
{% else %}
3. Follow the format and terminology of the project's existing docs.
{% endif %}
