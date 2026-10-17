---
name: plan
description: 开发计划：拆分需求、规划实施
category: 编程
params:
  risks:
    type: bool
    default: true
    description: 列出风险与未知项
  estimate:
    type: bool
    short: e
    description: 给出每一步的工作量估计
  steps:
    type: number
    min: 3
    max: 15
    description: 最多拆成几步
---
请为{% if extra %}「{{ extra }}」{% else %}这个需求{% endif %}制定实施计划，先不要写代码。

1. 先复述需求，列出你的理解和需要我确认的问题。
2. 调研现有代码，说明需要改动的模块和文件。
3. 拆分成{% if steps %}不超过 {{ steps }} 个{% endif %}可以独立验证的步骤，每一步说明改动内容和验证方式。
{% if estimate %}
4. 给出每一步的工作量估计。
{% endif %}
{% if risks %}
列出风险、未知项和可能的替代方案。
{% endif %}
等我确认计划后再开始实施。
