# Fonts

Roboto and Roboto Mono, by Christian Robertson for Google, Copyright 2011 Google Inc.
Licensed under the Apache License, Version 2.0: https://www.apache.org/licenses/LICENSE-2.0

Google's release files, copied from the sister project grandPA.

Arimo Regular and Bold (version 1.33), Copyright 2020 The Arimo Project Authors
(https://github.com/googlefonts/arimo). Licensed under the SIL Open Font License 1.1;
see `Arimo-OFL.txt`. Metric-compatible with Arial/Helvetica; sheets (PDF, SVG and the
Sheets mode) use it. Downloaded from the project's `fonts/ttf/` directory.

`DejaVuSans-Symbols.ttf` is a cut of DejaVu Sans 2.35 (Bitstream Vera licence, DejaVu changes
public domain; see `DejaVu-LICENSE.txt`) to arrows, misc technical (⌘ ⇧ ⌥ ⌃ ⏎ ⎋), geometric
shapes, dingbats and super/subscripts, made with
`pyftsubset DejaVuSans.ttf --unicodes="U+2190-21FF,U+2300-23FF,U+25A0-25FF,U+2600-26FF,U+2700-27BF,U+2070-209F,U+2026,U+00B7,U+00D7,U+2212,U+2022" --no-hinting --desubroutinize`.
The GUI puts it right behind Roboto. egui 0.37 has no emoji fonts in its defaults any more,
and egui_kittest panics on a missing glyph, so menu shortcuts (⌘K) need it.
