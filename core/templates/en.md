---
name: en
aliases: [英文]
description: 翻译成英文
category: 日常
params:
  style:
    type: enum
    positional: true
    description: 译文风格
    default: natural
    choices:
      - { value: natural, label: 自然地道 }
      - { value: technical, label: 技术文档 }
      - { value: casual, label: 口语 }
---
Translate the following into {% if style == "technical" %}clear, precise technical English suitable for documentation{% elif style == "casual" %}casual, conversational English{% else %}natural, idiomatic English{% endif %}. Keep code, identifiers and product names unchanged.
{% include "_extra" %}
