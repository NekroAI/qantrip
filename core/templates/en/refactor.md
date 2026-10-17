---
name: refactor
description: "Refactor: improve structure without changing behavior"
category: Write & fix
params:
  goal:
    type: multi
    positional: true
    description: What to improve
    choices:
      - { value: readability, label: Readability }
      - { value: structure, label: Module structure }
      - { value: naming, label: Naming }
      - { value: duplication, label: Remove duplication, aliases: [dup] }
      - { value: types, label: Types }
  keep-api:
    type: bool
    default: true
    description: Keep the public API unchanged
  mode:
    type: enum
    description: Execution mode
    default: suggest
    choices:
      - { value: suggest, label: Plan first }
      - { value: apply, label: Refactor directly }
---
Refactor {% if extra %}{{ extra }}{% else %}this code{% endif %}{% if goal %} to improve {% for g in goal %}{{ {"readability": "readability", "structure": "module structure", "naming": "naming", "duplication": "duplicated code", "types": "type design"}[g] }}{% if not loop.last %}, {% endif %}{% endfor %}{% endif %}.

Constraints:
1. Observable behavior must not change. Make sure tests cover it before refactoring; add them if they don't.
{% if keep_api %}
2. Keep the public API (function signatures, exports, configuration format) unchanged.
{% else %}
2. You may change the public API, but list every affected caller and update them together.
{% endif %}
3. Work in small steps that can each be verified.
4. Don't touch code unrelated to the goal.

{% include "_mode" %}
