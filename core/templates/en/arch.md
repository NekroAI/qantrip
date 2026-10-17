---
name: arch
description: Architecture review and technology choices
category: Understand & plan
params:
  focus:
    type: multi
    positional: true
    description: Criteria
    choices:
      - { value: scalability, label: Scalability }
      - { value: maintainability, label: Maintainability }
      - { value: cost, label: Cost }
      - { value: security, label: Security }
      - { value: complexity, label: Complexity }
  options:
    type: number
    min: 2
    max: 5
    default: 3
    description: Number of options to compare
---
Evaluate {% if extra %}"{{ extra }}"{% else %}the current architecture{% endif %}.

1. Describe the current state and constraints, separating known facts from assumptions.
2. Give {{ options }} viable options and compare them in a table on {% if focus %}{% for f in focus %}{{ {"scalability": "scalability", "maintainability": "maintainability", "cost": "cost", "security": "security", "complexity": "complexity"}[f] }}{% if not loop.last %}, {% endif %}{% endfor %}{% else %}maintainability, complexity, cost and risk{% endif %}.
3. Recommend one with reasons, and say under what conditions another option would be better.
4. List the main steps and risks of implementing the recommendation.

Analyze only; don't modify the code.
