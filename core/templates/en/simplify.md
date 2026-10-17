---
name: simplify
aliases: [simp]
description: Simplify code
category: Write & fix
params:
  scope:
    type: enum
    description: Scope
    default: changes
    choices:
      - { value: changes, label: All uncommitted changes }
      - { value: staged, label: Staged changes }
      - { value: branch, label: Current branch }
      - { value: last, label: Last commit }
  aggressive:
    type: bool
    short: a
    description: Allow larger simplifications
  mode:
    type: enum
    description: Execution mode
    default: suggest
    choices:
      - { value: suggest, label: Plan first }
      - { value: apply, label: Simplify directly }
---
Simplify the code in {% include "_scope" %}.

Look for duplicated logic, code that existing helpers could replace, unnecessary abstractions and indirection, overly defensive checks, and redundant comments.
{% if aggressive %}
You may merge or remove whole modules, but explain why and keep behavior unchanged.
{% else %}
Make local simplifications only; don't change the overall structure.
{% endif %}
Behavior must stay the same.

{% include "_mode" %}
{% include "_extra" %}
