\version "2.24.4"

\header {
  title = "ガラスのクレア"
  composer = "青木望"
  subtitle = "銀河鉄道999"
  dedication = "可憐な少女 / Galaxy Express 999 / Claire of Glass"
  poet = "Transcribed by Trdthg"
}

\paper {
  % Add space for instrument names
  indent = 10\mm
  markup-system-spacing.padding = #5
}

right = \relative c''' { 
  \clef treble
  \key c \major
  \time 4/4 
  \tempo 4 = 70
  \repeat volta 1 {
    | a8 ais c4 d c | 
    | g8 a ais4 c ais |
    | f8 g a4 ais a |
    | g8 f c4 s8 s c4 |
    | d8 e f4 s g4 |
    | e8 f a4 c d |
    | s e d d |
    | a c bes8 a g4 |
    | a8 ais c4 d c | 
    | g8 a ais4 c ais |
    | f8 g a4 ais a |
    | g8 f c4 s8 s s s |
    | c8 e f4 s g4 |
    | e8 f a4 c f |
    | s d8 a a a, a' d, |
    | d g, f' a, c g' <f a f'>4\arpeggio | 
  }
}


left = \relative c, {
  \clef bass 
  \repeat volta 1 {
    s4
    f8 c' f a
    e, c' e g
    d, b' d f
    cis, a' c f
    
    
    c, a' c f
    b,, a' d f
    c, c' e c
    c d e f

    d, a' d f
    cis, a' cis f
    c, a' c f
    b,, a' d f
    e, ais b d
    a, e' a  c
    
    g, d' f ais
    c, e g c
    
    % 第二节
    f , c' f a
    e, c' e g
    d, b' d f
    cis, a' c f
    
    c, a' c f
    a,, a' d f
    c, ais' d g
    g, c s s
    d, a' d f
    cis, a' cis f
    c, a' c f
    b,, a' d f
    c, ais' d s
    c, ais' d s
    f, a' c f 
    <c, a f f'>\arpeggio
    
  }
} 

\score {
  \new PianoStaff <<
    \new Staff = "RH" \right
    \new Staff = "LH" \left
  >>
  \layout { }
}

pianoMidi = {
  \new PianoStaff <<
    \new Staff = "RH" {
      \set Staff.midiInstrument = #"acoustic grand"
      \unfoldRepeats \right
    }
    \new Staff = "LH" {
      \set Staff.midiInstrument = #"acoustic grand"
      \left
    }
  >>
}

guitarMidi = {
  \new PianoStaff <<
    \new Staff = "RH" {
      \set Staff.midiInstrument = #"acoustic guitar (nylon)"
      \unfoldRepeats \right
    }
    \new Staff = "LH" {
      \set Staff.midiInstrument = #"acoustic guitar (nylon)"
      \left
    }
  >>
}

\book {
  \bookOutputName "claire-of-glass-piano"
  \score { \pianoMidi \midi { } }
}

\book {
  \bookOutputName "claire-of-glass-guitar"
  \score { \guitarMidi   \midi { } }
}