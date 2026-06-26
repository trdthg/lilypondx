---
title: "小星星 (Twinkle, Twinkle, Little Star)"
composer: "Traditional"
tempo: "4 = 100"
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
c4 c <e g> <e g> |
f4 f <e g> <e g> |
```

```lilypondx track=LH clef=bass relative=c,
c1 | c1 | f,1 | g'2 c,2 |
```
