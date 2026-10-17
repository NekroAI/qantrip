---
name: perf
description: 性能优化
category: 编程
params:
  focus:
    type: multi
    positional: true
    description: 关注的资源
    choices:
      - { value: cpu, label: CPU }
      - { value: memory, label: 内存, aliases: [mem] }
      - { value: io, label: I/O }
      - { value: latency, label: 响应延迟 }
      - { value: startup, label: 启动速度 }
  measure:
    type: bool
    default: true
    description: 先测量再优化
  mode:
    type: enum
    description: 执行模式
    default: suggest
    choices:
      - { value: read, label: 只分析瓶颈 }
      - { value: suggest, label: 给出优化方案 }
      - { value: apply, label: 直接优化 }
---
请分析并优化{% if extra %}{{ extra }}{% else %}这段代码{% endif %}的性能{% if focus %}，重点关注{% for f in focus %}{{ {"cpu": "CPU", "memory": "内存", "io": "I/O", "latency": "响应延迟", "startup": "启动速度"}[f] }}{% if not loop.last %}、{% endif %}{% endfor %}{% endif %}。

{% if measure %}
先测量：给出可复现的基准或剖析方法，用数据确认瓶颈在哪里，再决定优化什么。优化后用同样的方法对比结果。
{% endif %}
按收益排序给出优化点，说明每一项的预期收益、代价和风险。不要为了微小的收益牺牲可读性。

{% include "_mode" %}
