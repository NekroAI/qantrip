---
name: fix
description: 问题修复：找根因，最小修改
category: 编写与修复
params:
  mode:
    type: enum
    description: 执行模式
    default: apply
    choices:
      - { value: read, label: 只分析根因 }
      - { value: suggest, label: 给方案不改动 }
      - { value: apply, label: 直接修复 }
  test:
    type: bool
    short: t
    description: 先写能复现问题的测试
---
请修复下面描述的问题。
{% if extra %}

问题：{{ extra }}
{% endif %}

要求：
1. 先定位根因，说明问题为什么会发生，不要只处理表面症状。
{% if test %}
2. 修复前先写一个能复现问题的测试，确认它失败；修复后确认它通过。
{% else %}
2. 修复后说明如何验证问题已解决。
{% endif %}
3. 检查同样的根因是否在其他地方也存在。

{% include "_mode" %}
