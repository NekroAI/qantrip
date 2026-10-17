---
name: zh
aliases: [中文]
description: 翻译成中文
category: 日常
params:
  keep-terms:
    type: bool
    short: k
    description: 专业术语保留原文
  style:
    type: enum
    positional: true
    description: 译文风格
    default: natural
    choices:
      - { value: natural, label: 自然流畅 }
      - { value: literal, label: 贴近原文 }
      - { value: formal, label: 正式书面 }
---
帮我翻译成中文{% if style == "literal" %}，尽量贴近原文的结构和措辞{% elif style == "formal" %}，使用正式的书面语{% endif %}。
{% if keep_terms %}
专业术语、产品名和代码标识符保留原文，必要时在首次出现处用括号附中文。
{% endif %}
{% include "_extra" %}
