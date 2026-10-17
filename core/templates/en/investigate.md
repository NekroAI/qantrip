---
name: investigate
aliases: [inv]
description: "Investigate: gather evidence, weigh hypotheses"
category: Understand & plan
params:
  depth:
    type: enum
    positional: true
    description: Depth
    default: thorough
    choices:
      - { value: quick, label: Quick }
      - { value: thorough, label: Thorough }
  hypotheses:
    type: number
    min: 1
    max: 6
    default: 3
    description: Number of hypotheses
---
Investigate {% if extra %}{{ extra }}{% else %}this problem{% endif %}.

1. List the known facts and where they come from (code, logs, docs); don't treat guesses as facts.
2. Propose {{ hypotheses }} hypotheses, most likely first.
3. Gather evidence for and against each{% if depth == "quick" %}, checking only what matters most{% else %}, reading the relevant code, running commands or checking history where needed{% endif %}.
4. Give a conclusion, your confidence, and what else would be needed to confirm it.

Investigate only; don't modify any files.
