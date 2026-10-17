---
name: docs
description: 文档生成
category: 提交与文档
params:
  kind:
    type: enum
    positional: true
    description: 文档类型
    default: usage
    choices:
      - { value: api, label: API 文档 }
      - { value: readme, label: README }
      - { value: usage, label: 使用说明 }
      - { value: comments, label: 代码注释 }
      - { value: changelog, label: 更新日志 }
  audience:
    type: enum
    description: 读者
    default: developer
    choices:
      - { value: user, label: 最终用户 }
      - { value: developer, label: 开发者 }
---
请为{% if extra %}{{ extra }}{% else %}这部分代码{% endif %}编写{{ {"api": "API 文档", "readme": "README", "usage": "使用说明", "comments": "代码注释", "changelog": "更新日志"}[kind] }}，读者是{{ "最终用户" if audience == "user" else "开发者" }}。

要求：
1. 只写从代码中能确认的内容，不确定的地方标注出来问我。
2. 先给出最常见的用法示例，再讲细节。
{% if kind == "comments" %}
3. 注释解释"为什么"，不要复述代码在做什么；匹配项目现有的注释风格和密度。
{% else %}
3. 沿用项目现有文档的格式和术语。
{% endif %}
