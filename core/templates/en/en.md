---
name: en
aliases: [英文]
description: Translate into English
category: Translate
params:
  style:
    type: enum
    positional: true
    description: Translation style
    default: natural
    choices:
      - { value: natural, label: Natural }
      - { value: technical, label: Technical writing }
      - { value: casual, label: Casual }
---
Translate the following into {% if style == "technical" %}clear, precise technical English suitable for documentation{% elif style == "casual" %}casual, conversational English{% else %}natural, idiomatic English{% endif %}. Keep code, identifiers and product names unchanged.
{% include "_extra" %}
