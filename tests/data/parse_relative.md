---
title: "parse: relative pitch basics"
---

# Relative pitch basics

Anchor `c'` (C4=60). Each note takes the closest octave to the previous.

```lilypond track=T clef=treble relative=c'
a8 ais c4 d c
```

```lilypond-test
57,240 58,240 60,480 62,480 60,480
```
