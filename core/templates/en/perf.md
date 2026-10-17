---
name: perf
description: Performance optimization
category: Write & fix
params:
  focus:
    type: multi
    positional: true
    description: Resources to focus on
    choices:
      - { value: cpu, label: CPU }
      - { value: memory, label: Memory, aliases: [mem] }
      - { value: io, label: I/O }
      - { value: latency, label: Latency }
      - { value: startup, label: Startup time }
  measure:
    type: bool
    default: true
    description: Measure before optimizing
  mode:
    type: enum
    description: Execution mode
    default: suggest
    choices:
      - { value: read, label: Find bottlenecks only }
      - { value: suggest, label: Propose optimizations }
      - { value: apply, label: Optimize directly }
---
Analyze and improve the performance of {% if extra %}{{ extra }}{% else %}this code{% endif %}{% if focus %}, focusing on {% for f in focus %}{{ {"cpu": "CPU", "memory": "memory", "io": "I/O", "latency": "latency", "startup": "startup time"}[f] }}{% if not loop.last %}, {% endif %}{% endfor %}{% endif %}.

{% if measure %}
Measure first: give a reproducible benchmark or profiling method, use the data to confirm where the bottleneck is, then decide what to optimize. Compare with the same method afterwards.
{% endif %}
List optimizations by payoff, with the expected gain, cost and risk of each. Don't trade readability for small gains.

{% include "_mode" %}
