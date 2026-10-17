---
name: plan
description: "Implementation plan: break down the work"
category: Understand & plan
params:
  risks:
    type: bool
    default: true
    description: List risks and unknowns
  estimate:
    type: bool
    short: e
    description: Estimate each step
  steps:
    type: number
    min: 3
    max: 15
    description: Maximum number of steps
---
Make an implementation plan for {% if extra %}"{{ extra }}"{% else %}this requirement{% endif %}. Don't write code yet.

1. Restate the requirement, listing your understanding and the questions I need to answer.
2. Study the existing code and name the modules and files that need to change.
3. Break the work into {% if steps %}at most {{ steps }} {% endif %}steps that can each be verified, with what changes and how to verify it.
{% if estimate %}
4. Estimate the effort for each step.
{% endif %}
{% if risks %}
List risks, unknowns and possible alternatives.
{% endif %}
Wait for me to confirm the plan before starting.
