---
name: investigate
aliases: [inv]
description: 问题调查：收集证据、评估假设
category: 编程
params:
  depth:
    type: enum
    positional: true
    description: 调查深度
    default: thorough
    choices:
      - { value: quick, label: 快速 }
      - { value: thorough, label: 深入 }
  hypotheses:
    type: number
    min: 1
    max: 6
    default: 3
    description: 列出的假设数量
---
请调查{% if extra %}：{{ extra }}{% else %}这个问题{% endif %}

1. 列出已知事实和来源（代码、日志、文档），不要把推测当事实。
2. 提出 {{ hypotheses }} 个假设，按可能性排序。
3. 为每个假设收集支持和反对的证据{% if depth == "quick" %}，只做最关键的检查{% else %}，必要时阅读相关代码、运行命令或查看历史提交{% endif %}。
4. 给出结论、置信度，以及还需要哪些信息才能确认。

只调查，不要修改任何文件。
