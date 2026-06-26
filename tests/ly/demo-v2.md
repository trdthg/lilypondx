---
title: ""
composer: "Traditional"
tempo: "4 = 120"
key: 'c \major'
time: "4/4"
---

# 小星星

经典的启蒙旋律，用于演示 `lilypondx` 的解析、渲染与播放。
右手 `relative=c'` 锚定 C4; 第一个 `g'` 用八度标记强制上跳，
之后 relative 上下文留在 G4，后续裸 `g` 自然继承。
左手 `relative=c,` 锚定 C2，采用 I-I-IV-V-I 进行，
最后一小节 `g'2 c,2` 形成 G→C 的完满终止 (属到主)。

```lilypond track=RH clef=treble relative=c'
c4 d <e g>2 <e g>4 <e g>
f4 e <b d>2 <b d>4 <b d>
e4 d <a c>2 <a c>4 <a c>
a4 c <g b>2 <g b>4 <g b>

a4 c <d g>2 <d g>4 <d g>
e4 f <g b>2 <g b>4 <g b>
c4 b <a a,>2 <a a,>4 <a a,>
c,4 d <g, b>2 <g b>4 <g b>
```

```lilypondx track=LH clef=bass relative=c,
```
