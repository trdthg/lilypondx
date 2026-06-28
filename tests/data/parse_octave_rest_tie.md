---
title: "parse: rests, ties, dotted"
---

# Rests, ties, dotted durations

```lilypond track=T clef=treble relative=c'
c4 r8 d4. c4~ c8
```

```lilypond-test
60,480 R,240 62,720 60,720
```

```lilypond track=T2 clef=treble relative=c'
c4 c8 c4 c8
```

```lilypond-test
60,480 60,240 60,480 60,240
```
