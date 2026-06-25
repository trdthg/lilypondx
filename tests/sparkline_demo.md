# ASCII Sparkline Demo

两个字符：`•`（音头 U+2022）`━`（时值 U+2501）。无连线，音符各自落在对应音高行上。


## 1. 上行

```lilypondx track=M clef=treble relative=c'
c4 d e f g a b c'
```

```
 C#6                •━
 A#5                 
  G5                 
  E5                 
 C#5                 
  B4              •━ 
 G#4          •━•━ 
  F4      •━•━ 
  D4    •━ 
  B3  •━ 
      ━━━━━━━━━━━━━━━━
```

---

## 2. 下行

```lilypondx track=M clef=treble relative=c''
c'' b a g f e d c
```

```
 C#7   
 A#6   
  G6   
  E6   
 C#6   
  B5  • 
 G#5   • 
  F5    • 
  D5      
  B4     •
      ━━━━
```

---

## 3. 上下混合

```lilypondx track=M clef=treble relative=c'
c4 e g c' g e c
```

```
 C#6        •━ 
 A#5           
  G5          •━ 
  E5            •━ 
 C#5               
  B4              •━
 G#4      •━ 
  F4    •━ 
  D4     
  B3  •━ 
      ━━━━━━━━━━━━━━
```

---

## 4. 不同时值

```lilypondx track=M clef=treble relative=c'
c2 d4 e8 f g2
```

```
 G#4  
  G4          •━━━
 F#4           
  F4         • 
  E4        • 
 D#4         
  D4      •━ 
 C#4       
  C4  •━━━ 
  B3  
      ━━━━━━━━━━━━
```

---

## 5. 跳进

```lilypondx track=M clef=treble relative=c'
c4 g c' g, c g c'
```

```
 C#5      •━      •━
  B4               
  A4               
  G4               
  F4               
  D4               
  C4  •━      •━   
 A#3               
 G#3               
 F#3    •━  •━  •━ 
      ━━━━━━━━━━━━━━
```

---

## 6. 同音反复

```lilypondx track=M clef=treble relative=c'
c4 c c c d d e e
```

```
  C4  •━•━•━•━ 
  B3  
      ━━━━━━━━━━
```

---

## 7. 休止符

```lilypondx track=M clef=treble relative=c'
c4 r d r e r f r
```

```
  F4              •━
  E4               
  E4          •━   
 D#4           
  D4      •━   
 C#4                   (×2)
  C4  •━   
  B3  
      ━━━━━━━━━━━━━━━━
```

---

## 8. 连线 tie

```lilypondx track=M clef=treble relative=c'
c4~ c d~ d e~ e f
```

```
  F4              •━
  E4               
  E4          ━━•━ 
 D#4           
  D4      ━━•━ 
 C#4                   (×2)
  C4  •━•━ 
  B3  
      ━━━━━━━━━━━━━━
```

---

## 9. 双声部

```lilypondx track=RH clef=treble relative=c'
c4 e g c'
```

```lilypondx track=LH clef=bass relative=c,
c4 c g, g,
```

```
=== RH ===
 C#6        •━
 A#5         
  G5         
  E5         
 C#5         
  B4         
 G#4      •━ 
  F4    •━ 
  D4     
  B3  •━ 
      ━━━━━━━━

=== LH ===
 C#2  •━•━ 
  B1       
  A1       
  G1       
  F1       
  D1       
  C1       
 A#0       
 G#0       
 F#0      •━•━
      ━━━━━━━━
```


---

## 10. 测试专用块

```lilypond-test track=T clef=treble relative=c'
c4 d e f | g a b c'
```

> `lilypond-test` 块不会出现在 watch TUI 中，只用于自动化测试。
