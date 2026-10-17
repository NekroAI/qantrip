---
name: arch
description: 架构评估与技术选型
category: 编程
params:
  focus:
    type: multi
    positional: true
    description: 评估维度
    choices:
      - { value: scalability, label: 可扩展性 }
      - { value: maintainability, label: 可维护性 }
      - { value: cost, label: 成本 }
      - { value: security, label: 安全 }
      - { value: complexity, label: 复杂度 }
  options:
    type: number
    min: 2
    max: 5
    default: 3
    description: 比较的方案数量
---
请评估{% if extra %}「{{ extra }}」{% else %}当前的架构设计{% endif %}。

1. 先说明现状和约束条件，区分已知事实和假设。
2. 给出 {{ options }} 个可行方案，用表格比较{% if focus %}{% for f in focus %}{{ {"scalability": "可扩展性", "maintainability": "可维护性", "cost": "成本", "security": "安全", "complexity": "复杂度"}[f] }}{% if not loop.last %}、{% endif %}{% endfor %}{% else %}可维护性、复杂度、成本和风险{% endif %}。
3. 给出推荐方案和理由，以及在什么条件下应该改选其他方案。
4. 列出实施推荐方案的主要步骤和风险。

只做分析，不要修改代码。
