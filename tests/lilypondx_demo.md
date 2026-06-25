---
title: "测试 lilypondx 语法"
composer: "Trdthg"
tempo: "4 = 120"
key: 'c \major'
time: "4/4"
---

# lilypondx 简化语法示例

这个文件使用 `lilypondx` 语法块，会被我们的解析器处理并生成走势图。

## 主旋律 (lilypondx)

```lilypondx track=RH clef=treble relative=c'
c4 d e f | g a b c' | c' b a g | f e d c
```

## 低音 (lilypondx)

```lilypondx track=LH clef=bass relative=c,
c4 c g g | a a g f | e e d d | c c g g
```
