---
title: "pipeline: multi-track lilypondx"
composer: "Trdthg"
tempo: "4 = 120"
key: 'c \major'
time: "4/4"
---

# Multi-track pipeline smoke test

Uses `lilypondx` syntax (parsed by our engine, not passed to LilyPond).

```lilypondx track=RH clef=treble relative=c'
c4 d e f | g a b c'
```

```lilypondx track=LH clef=bass relative=c,
c4 c g, g, | a, a, g, f,
```
