---
title: "parse: s-rest and bar check"
---

# `s` rests, bar checks (no longer tracked but should parse), comments

```lilypond track=T clef=treble relative=c
c4 s4 d4 | % comment here
e4 f4
```

```lilypond-test
48,480 R,480 50,480 52,480 53,480
```
