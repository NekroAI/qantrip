---
name: zh
aliases: [中文]
description: Translate into Chinese
category: Translate
params:
  keep-terms:
    type: bool
    short: k
    description: Keep technical terms in the original language
  style:
    type: enum
    positional: true
    description: Translation style
    default: natural
    choices:
      - { value: natural, label: Natural }
      - { value: literal, label: Close to the original }
      - { value: formal, label: Formal }
---
Translate the following into Simplified Chinese{% if style == "literal" %}, staying close to the original structure and wording{% elif style == "formal" %}, using a formal written register{% endif %}.
{% if keep_terms %}
Keep technical terms, product names and code identifiers in the original language; where helpful, add the Chinese in parentheses on first use.
{% endif %}
{% include "_extra" %}
