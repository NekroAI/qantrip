---
name: refactor
description: 代码重构：改善结构，行为不变
category: 编写与修复
params:
  goal:
    type: multi
    positional: true
    description: 重构目标
    choices:
      - { value: readability, label: 可读性 }
      - { value: structure, label: 模块结构 }
      - { value: naming, label: 命名 }
      - { value: duplication, label: 消除重复, aliases: [dup] }
      - { value: types, label: 类型 }
  keep-api:
    type: bool
    default: true
    description: 保持公共接口不变
  mode:
    type: enum
    description: 执行模式
    default: suggest
    choices:
      - { value: suggest, label: 先给方案 }
      - { value: apply, label: 直接重构 }
---
请重构{% if extra %}{{ extra }}{% else %}这部分代码{% endif %}{% if goal %}，目标是改善{% for g in goal %}{{ {"readability": "可读性", "structure": "模块结构", "naming": "命名", "duplication": "重复代码", "types": "类型设计"}[g] }}{% if not loop.last %}、{% endif %}{% endfor %}{% endif %}。

约束：
1. 外部可观察的行为必须完全不变；重构前确认有测试覆盖，没有的先补上。
{% if keep_api %}
2. 保持公共接口（函数签名、导出、配置格式）不变。
{% else %}
2. 允许调整公共接口，但要列出所有受影响的调用方并一起修改。
{% endif %}
3. 分小步进行，每一步都可以单独验证。
4. 不要顺带修改与目标无关的代码。

{% include "_mode" %}
