# Third-party

## Equalizer APO

- Autor / Author: Jonas Thedering — https://sourceforge.net/projects/equalizerapo/
- Licencia / License: GPL-2.0
- PoxiEQ no incluye ni modifica Equalizer APO: descarga su instalador oficial (versión 1.4.2, SHA-256
  `7403be7427bbe1936a40dded082829b6e217fc4f5990fee5cba501f0ae055afa`), lo ejecuta y escribe en su carpeta de
  configuración. / PoxiEQ does not bundle or modify Equalizer APO: it downloads the official installer (checked by
  SHA-256), runs it and writes to its configuration folder.

## bs2b

- Autor / Author: Boris Mikhaylov — https://bs2b.sourceforge.net/
- Licencia / License: MIT
- El crossfeed de PoxiDSP usa los filtros de bs2b reescritos en Rust. / PoxiDSP's crossfeed uses bs2b's filters,
  rewritten in Rust.

## MIT KEMAR HRTF

- Autores / Authors: Bill Gardner y / and Keith Martin, MIT Media Lab Machine Listening Group — «HRTF Measurements of a
  KEMAR Dummy-Head Microphone» (1994), https://sound.media.mit.edu/resources/KEMAR.html
- Licencia / License: «This data is Copyright 1994 by the MIT Media Laboratory. It is provided free with no restrictions
  on use, provided the authors are cited when the data is used in any research or commercial application.»
- Los altavoces virtuales de PoxiDSP usan las respuestas a 30° y 0° de elevación del juego «diffuse-field equalized»,
  sin el retardo común y con la coloración del centro compensada (`scripts/hrir.mjs` → `dsp/src/hrir.rs`). / PoxiDSP's
  virtual speakers use the 30°, 0° elevation responses from the diffuse-field equalized set, with the common delay
  removed and the center coloration compensated.

## Otras / Others

Tauri, React, Zustand, lucide-react, Nunito (Fontsource), winreg, windows-rs, reqwest, sha2 y serde, cada una con su
licencia MIT / Apache-2.0 / ISC / OFL. / Each under its own MIT / Apache-2.0 / ISC / OFL license.
