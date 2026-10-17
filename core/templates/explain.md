---
name: explain
aliases: [ex]
description: 代码解释
category: 编程
params:
  level:
    type: enum
    positional: true
    description: 讲解深度
    default: detailed
    choices:
      - { value: brief, label: 一句话概括 }
      - { value: detailed, label: 详细讲解 }
      - { value: beginner, label: 面向新手 }
  diagram:
    type: bool
    short: d
    description: 附带 Mermaid 流程图
---
请解释{% if extra %}{{ extra }}{% else %}这段代码{% endif %}。

{% if level == "brief" %}
用几句话说明它做什么、为什么这样做。
{% elif level == "beginner" %}
假设我不熟悉这个代码库和相关技术：先讲它解决什么问题，再按执行顺序讲关键步骤，遇到术语时简单解释。
{% else %}
包括：它解决什么问题；主要流程和数据流；关键设计决策及其原因；容易出错或不直观的地方。
{% endif %}
{% if diagram %}
附一张 Mermaid 流程图说明主要流程。
{% endif %}
只解释，不要修改代码。
