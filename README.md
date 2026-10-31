<div align="center">

<img src=".assets/banner.png" alt="PoxiEQ" width="100%" />

<br />

![Versión](https://img.shields.io/badge/versión-0.1.0-e879f9?style=flat-square)
![Plataforma](https://img.shields.io/badge/Windows-10%20%7C%2011-34d399?style=flat-square)
![Latencia](https://img.shields.io/badge/latencia-cero-ffd25f?style=flat-square)
![Motor](https://img.shields.io/badge/motor-Equalizer%20APO-38bdf8?style=flat-square)
![Licencia](https://img.shields.io/badge/licencia-PolyForm%20Noncommercial-ff6ec7?style=flat-square)

**🇪🇸 Español** · [🇬🇧 English](#-english)

### Ecualiza todo el sonido de Windows. Sin reiniciar, sin retraso, sin líos.

</div>

---

## 🎯 Qué es

**PoxiEQ** es un ecualizador para la salida de audio de Windows: lo que ajustes se aplica a **todo** lo que suena
(juegos, música, Discord, el navegador…). Mueves los puntos de la curva con el ratón y el sonido cambia al instante.

Por dentro usa **Equalizer APO**, un motor gratuito y de código abierto que se mete dentro del audio de Windows.
PoxiEQ lo instala y lo activa por ti con un clic, **sin reiniciar el PC**.

## ✨ Qué hace

- ⚡ **Activación en un clic**: descarga el motor oficial (comprobando su firma SHA-256), lo instala en silencio y lo engancha a tus cascos o altavoces. Solo se corta el sonido un par de segundos.
- 🖱️ **Curva en vivo**: arrastra los puntos, la **rueda** cambia el ancho (Q), **doble clic** añade una banda y el **clic derecho** cambia el tipo (campana, graves, agudos, paso bajo, paso alto).
- 📊 **Analizador en tiempo real** detrás de la curva: espectro con retención de picos o espectrograma, para ver lo que suena mientras ecualizas.
- 🎧 **AutoEq**: pega la corrección de tu modelo de cascos (miles en [autoeq.app](https://autoeq.app)) y listo.
- 🛡️ **Preamp automático**: baja lo justo para que la curva nunca sature.
- 🔊 **Dinámica**: sonoridad igualada con un clic (Streaming −14 LUFS, Fuerte −9, Noche −23 para pelis sin molestar), limitador que nunca pasa de −1 dBTP, compresor, graves psicoacústicos, realce de diálogos y crossfeed para cascos. Con todo apagado no toca ni un bit.
- 🎧 **Altavoces virtuales**: con cascos, el estéreo suena desde dos altavoces delante de ti (±30°) en vez de dentro de la cabeza. Convolución con respuestas de cabeza reales (KEMAR del MIT) e intensidad regulable sin cambiar el timbre del centro.
- 🎛️ **Herramientas de estudio**: mono, side, solo L / R, invertir polaridad e intercambiar canales, exactos y sin colorear. Y simulación de móvil, portátil, coche, auriculares de botón, tele y altavoz bluetooth para comprobar cómo traduce una mezcla. Momentáneas: se ve un aviso mientras están puestas y se quitan al cerrar la ventana.
- 👂 **Test de audición**: mide en cada oído tu umbral en 10 frecuencias y crea el preset «Mi audición», una compensación suave (también por oído). No es una prueba médica.
- 🎚️ **Presets**: Plano, Más graves, Voz clara, Pasos (gaming), Cine, Música, Noche… y los tuyos. Compártelos con un código para pegar en un chat o como archivo `.poxieq` (arrástralo a la ventana para importarlo).
- 🎯 **Preset por aplicación**: Spotify → Música, tu juego → Pasos. Cambia solo cuando la app pasa a primer plano y vuelve al que tenías al salir.
- ⌨️ **Atajos de presets**: `Ctrl+Alt+1…9`, con un aviso en pantalla que no roba el foco (también desde la bandeja).
- ↩️ **Deshacer y rehacer** (`Ctrl+Z` / `Ctrl+Y`) de la curva y la dinámica: un arrastre entero es un solo paso.
- 🔌 **Un EQ por salida**: cada dispositivo guarda su propia curva.
- ⏯️ **Bypass** con **Ctrl+Alt+E** desde cualquier sitio (también en juegos) y desde la bandeja.
- 🎨 Temas oscuro y claro, 7 colores, español e inglés. Arranca con Windows si quieres y se actualiza solo.

## 📥 Descarga

Busca la última versión en **[Releases](../../releases/latest)**:

| Archivo | Para qué |
|---|---|
| `PoxiEQ-Setup-x.y.z.exe` | **Instalador**. Se actualiza solo |
| `PoxiEQ-Portable-x.y.z.exe` | **Portable**. Se ejecuta tal cual |

Requiere Windows 10 u 11 (64 bits). La primera vez, PoxiEQ te ofrece instalar el motor: Windows pedirá permiso de
administrador una sola vez.

> 🛡️ Si Windows dice «Windows protegió tu PC», pulsa **Más información → Ejecutar de todas formas**. Sale con
> todos los programas nuevos sin firma de pago; el código está aquí mismo.

## ❓ Preguntas

- **¿Si cierro PoxiEQ deja de ecualizar?** No. El motor sigue aplicando la última curva. PoxiEQ solo hace falta para cambiarla.
- **¿Cómo lo quito de un dispositivo?** Ajustes → Motor de audio → apaga esa salida → Aplicar. Se restauran los efectos originales del dispositivo.
- **¿Y si ya usaba Equalizer APO?** PoxiEQ guarda tu `config.txt` como `config.antes-de-poxieq.txt` antes de tocarlo.

## 🛠️ Desarrollo

Tauri 2 + Rust (motor, registro de Windows, analizador) y React + TypeScript (la interfaz).

- `start.bat`: modo desarrollo.
- `deploy.bat`: instalador firmado + portable + `latest.json` en `deploy-hosting\`.
- `cd src-tauri && cargo test`, `cd dsp && cargo test` y `npm test`: tests del núcleo (también las reglas por app y las herramientas de estudio), de la dinámica y los altavoces virtuales (PoxiDSP, el plugin propio que carga el motor), de la matemática del EQ y del test de audición, del historial de deshacer y del formato para compartir presets.
- `node scripts/hrir.mjs <elev0/H0e030a.wav>`: regenera `dsp/src/hrir.rs` desde el `diffuse.zip` del KEMAR del MIT.

## 📄 Licencia

[PolyForm Noncommercial 1.0.0](LICENSE): úsalo, estúdialo y modifícalo gratis para uso no comercial.
Equalizer APO es de Jonas Thedering (GPL-2.0); las respuestas de cabeza de los altavoces virtuales son del KEMAR del
MIT Media Lab (Bill Gardner y Keith Martin); ver [THIRD-PARTY.md](THIRD-PARTY.md).

---

## 🇬🇧 English

**PoxiEQ** is an equalizer for the Windows audio output: whatever you set applies to **everything** that plays
(games, music, Discord, the browser…). Drag the points on the curve and the sound changes instantly.

Under the hood it uses **Equalizer APO**, a free and open source engine that plugs into Windows audio. PoxiEQ installs
and enables it for you with one click, **without restarting your PC**.

### ✨ Features

- ⚡ **One-click setup**: downloads the official engine (checking its SHA-256), installs it silently and hooks it to your headphones or speakers. The sound only cuts out for a couple of seconds.
- 🖱️ **Live curve**: drag the points, the **wheel** changes the width (Q), **double click** adds a band and **right click** changes the type (bell, low shelf, high shelf, low pass, high pass).
- 📊 **Real-time analyzer** behind the curve: spectrum with peak hold, or spectrogram.
- 🎧 **AutoEq**: paste the correction for your headphone model (thousands at [autoeq.app](https://autoeq.app)).
- 🛡️ **Automatic preamp** so the curve never clips.
- 🔊 **Dynamics**: one-click loudness matching (Streaming −14 LUFS, Loud −9, Night −23 for movies without disturbing anyone), a limiter that never goes above −1 dBTP, compressor, psychoacoustic bass, dialogue enhancer and headphone crossfeed. With everything off it doesn't touch a single bit.
- 🎧 **Virtual speakers**: on headphones, stereo plays from two speakers in front of you (±30°) instead of inside your head. Convolution with real head responses (MIT KEMAR) and adjustable intensity without changing the tone of the center.
- 🎛️ **Studio tools**: mono, side, solo L / R, polarity invert and channel swap, exact and uncolored. Plus phone, laptop, car, earbuds, TV and Bluetooth speaker simulation to check how a mix translates. Momentary: a notice shows while they're on and they turn off when you close the window.
- 👂 **Hearing test**: measures your threshold at 10 frequencies in each ear and creates the "My hearing" preset, a gentle compensation (per ear too). Not a medical test.
- 🎚️ **Presets**: Flat, More bass, Clear voice, Footsteps (gaming), Cinema, Music, Night… and your own. Share them as a code to paste in a chat or as a `.poxieq` file (drop it on the window to import it).
- 🎯 **Preset per app**: Spotify → Music, your game → Footsteps. It switches when the app comes to the foreground and goes back to yours when you leave.
- ⌨️ **Preset shortcuts**: `Ctrl+Alt+1…9`, with an on-screen notice that doesn't steal focus (also from the tray).
- ↩️ **Undo and redo** (`Ctrl+Z` / `Ctrl+Y`) for the curve and dynamics: a whole drag is a single step.
- 🔌 **One EQ per output**: each device keeps its own curve.
- ⏯️ **Bypass** with **Ctrl+Alt+E** from anywhere (games included) and from the tray.
- 🎨 Dark and light themes, 7 colors, Spanish and English. Optional start with Windows, updates itself.

### 📥 Download

Get the latest version from **[Releases](../../releases/latest)**: `PoxiEQ-Setup-x.y.z.exe` (installer, updates
itself) or `PoxiEQ-Portable-x.y.z.exe` (portable). Windows 10 or 11, 64-bit.

### ❓ FAQ

- **Does the EQ stop if I close PoxiEQ?** No. The engine keeps applying the last curve; PoxiEQ is only needed to change it.
- **How do I remove it from a device?** Settings → Audio engine → turn that output off → Apply. The device's original effects are restored.
- **I already used Equalizer APO.** PoxiEQ saves your `config.txt` as `config.antes-de-poxieq.txt` before touching it.

### 📄 License

[PolyForm Noncommercial 1.0.0](LICENSE). Equalizer APO is by Jonas Thedering (GPL-2.0); the virtual speakers' head
responses are from the MIT Media Lab KEMAR (Bill Gardner and Keith Martin); see [THIRD-PARTY.md](THIRD-PARTY.md).
