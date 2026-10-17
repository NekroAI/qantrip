---
name: simplify
aliases: [simp]
description: 代码简化
category: 编写与修复
params:
  scope:
    type: enum
    description: 范围
    default: changes
    choices:
      - { value: changes, label: 所有未提交改动 }
      - { value: staged, label: 暂存区 }
      - { value: branch, label: 当前分支 }
      - { value: last, label: 最近一次提交 }
  aggressive:
    type: bool
    short: a
    description: 允许较大幅度的简化
  mode:
    type: enum
    description: 执行模式
    default: suggest
    choices:
      - { value: suggest, label: 先给方案 }
      - { value: apply, label: 直接简化 }
---
请简化{% include "_scope" %}中的代码。

寻找：重复的逻辑、可以用现有工具函数替代的代码、不必要的抽象和间接层、过度防御的检查、冗余的注释。
{% if aggressive %}
可以合并或删除整个模块，但要说明理由并保证行为不变。
{% else %}
只做局部简化，不改变整体结构。
{% endif %}
行为必须保持不变。

{% include "_mode" %}
{% include "_extra" %}
