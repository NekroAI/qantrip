---
name: pr
description: 生成 PR 描述
category: 提交与文档
params:
  sections:
    type: multi
    description: 包含的章节
    default: [summary, changes, testing]
    choices:
      - { value: summary, label: 概述 }
      - { value: changes, label: 改动列表 }
      - { value: testing, label: 测试情况 }
      - { value: risks, label: 风险与影响 }
      - { value: followup, label: 后续工作 }
  lang:
    type: enum
    description: 语言
    default: en
    choices:
      - { value: en, label: 英文 }
      - { value: zh, label: 中文 }
  create:
    type: bool
    description: 用 gh 直接创建 PR
---
请根据当前分支相对主分支的全部改动，用{{ "中文" if lang == "zh" else "英文" }}写一份 PR 描述，包含以下章节：
{% for s in sections %}
- {{ {"summary": "概述：这个 PR 解决什么问题，为什么需要", "changes": "改动列表：按模块归纳主要改动", "testing": "测试情况：如何验证，覆盖了哪些场景", "risks": "风险与影响：兼容性、迁移、性能等", "followup": "后续工作：有意未在本 PR 处理的事项"}[s] }}
{% endfor %}

只写从改动中能确认的内容。{% if create %}写完后用 gh pr create 创建 PR。{% else %}只输出描述，不要创建 PR。{% endif %}
{% include "_extra" %}
