# Changelog

## 0.2.0

### 🇪🇸 Español

- 🔌 **Tus plugins VST2 y VST3** en la cadena de cada salida o micrófono: añádelos desde su archivo (o busca todos los de tus carpetas), ordénalos, apágalos y abre su ventana de verdad, la del plugin que suena, con sus medidores en vivo. Su latencia se suma al medidor.
- 🩺 **¿Por qué no suena?**: en Ajustes → Motor, un diagnóstico que mira audio espacial, apps en exclusivo, silencio, otra salida predeterminada o efectos del fabricante, y te dice qué hacer.
- 🛠️ **Se repara solo**: si una actualización de Windows o del driver quita el motor de una salida, PoxiEQ lo detecta y te ofrece repararlo con un clic. Si alguien toca su configuración, la rehace.
- 🎧 **Bluetooth completo**: activa a la vez el modo música y el de llamadas de tus cascos, y la curva también suena en las llamadas.
- 🎯 **Apps**: página propia con la salida de audio de cada app (Spotify a los altavoces, el juego a los cascos), su preset, atenuación automática cuando habla Discord y un mezclador juego/chat con atajos.
- 🎙️ **Supresión de ruido** en el micrófono (RNNoise): quita teclado, ventilador y ruido de fondo antes de que llegue a Discord u OBS.
- 🌀 **Convolución**: carga un WAV de respuesta al impulso (BRIR de HeSuVi/Impulcifer, FIR de REW) en cualquier salida, con su respuesta dibujada.
- 🎯 **Objetivos de AutoEq** y **«Suena como»**: lleva tus cascos a otra curva objetivo o a que suenen como otro modelo.
- 📐 **Sala**: carga la calibración de tu micrófono de medida y avisa si una toma sale rara.
- 🎛️ **Estudio**: simulación de tablet, barra de sonido, monitores de estudio y club, e inclinación de tono ±6 dB.
- ⚖️ **Balance** izquierda/derecha por salida.
- 📄 **Importar y exportar** curvas de REW, Peace, EasyEffects, AutoEq y GraphicEQ.
- ⚡ **Activar solos** los dispositivos nuevos que conectes, sin volver a pedir permiso.
- 📊 **Mini-barra** siempre encima con el medidor, desde la bandeja, que no roba el foco.

### 🇬🇧 English

- 🔌 **Your VST2 and VST3 plugins** in the chain of each output or microphone: add them from their file (or scan your folders), reorder, turn off and open their real window, the one of the plugin you hear, with live meters. Their latency adds to the meter.
- 🩺 **Why is there no sound?**: in Settings → Engine, a diagnosis that checks spatial audio, exclusive-mode apps, mute, another default output or vendor effects, and tells you what to do.
- 🛠️ **Self-repair**: if a Windows or driver update removes the engine from an output, PoxiEQ notices and offers a one-click repair. If something touches its configuration, it rewrites it.
- 🎧 **Full Bluetooth**: enables both the music and call modes of your headphones at once, and the curve plays in calls too.
- 🎯 **Apps**: its own page with each app's audio output (Spotify to the speakers, the game to the headphones), its preset, automatic ducking when Discord talks and a game/chat mixer with shortcuts.
- 🎙️ **Noise suppression** on the microphone (RNNoise): removes keyboard, fan and background noise before it reaches Discord or OBS.
- 🌀 **Convolution**: load an impulse response WAV (HeSuVi/Impulcifer BRIR, REW FIR) on any output, with its response drawn.
- 🎯 **AutoEq targets** and **"Sounds like"**: bring your headphones to another target curve or make them sound like another model.
- 📐 **Room**: load your measurement microphone's calibration and get a warning when a take looks off.
- 🎛️ **Studio**: tablet, soundbar, studio monitors and club simulation, plus ±6 dB tone tilt.
- ⚖️ **Left/right balance** per output.
- 📄 **Import and export** curves from REW, Peace, EasyEffects, AutoEq and GraphicEQ.
- ⚡ **Auto-enable** new devices you plug in, without asking for permission again.
- 📊 **Mini bar** always on top with the meter, from the tray, that doesn't steal focus.

## 0.1.0

### 🇪🇸 Español

- 🎚️ **Ecualizador para todo Windows**: juegos, música, llamadas y navegador, sin latencia.
- 🧭 **Todo por tareas**: menú lateral con Ecualizador, Sonido, Cascos, Estudio, Sala y Micrófono. Nada escondido.
- 🔁 **Sigue la salida de Windows**: al conectar unos cascos o un altavoz Bluetooth, PoxiEQ cambia solo a su EQ.
- ⚡ **Activación en un clic y sin reiniciar**: PoxiEQ instala su motor y lo activa en tus cascos o altavoces.
- 🖱️ **Curva en vivo**: arrastra los puntos, la rueda cambia el ancho y el sonido cambia al instante.
- 🎧 **Buscar mis cascos**: escribe tu modelo y aplica su corrección de AutoEq.
- 🛡️ **Preamp automático** para que nunca sature, **bypass** con `Ctrl+Alt+Num 0` y desde la bandeja.
- 🔊 **Dinámica**: presets de sonoridad (Natural, Streaming, Fuerte, Noche), limitador de true peak, compresor, nivelador por LUFS, armónicos de graves, realce de diálogos y crossfeed.
- 🔈 **Graves con una rueda**: hasta +10 dB de graves limpios y con pegada (sin embarrar los medios ni distorsionar) y protección para que nunca sature.
- 🎯 **Preset por aplicación**: cada app con su preset (Spotify → Música, un juego → Pasos) y, al salir, vuelve el que tenías.
- ⌨️ **Atajos a tu gusto**: grábalos en Ajustes (por defecto `Ctrl+Alt+Num 0…9`), con un aviso en pantalla que no roba el foco. No deja usar los que chocan con AltGr (`€ | @ # ~`) y avisa si otro programa ya tiene el atajo.
- 🗂️ **Presets en la bandeja**: cambia de preset sin abrir la ventana.
- ↩️ **Deshacer y rehacer**: `Ctrl+Z` y `Ctrl+Y` en la curva y la dinámica.
- 💾 **Presets en todas las secciones**: guarda y cambia con un clic tu Sonido, tu escucha con Cascos, tus comprobaciones de Estudio y los ajustes del Micrófono, igual que las curvas del Ecualizador.
- 🎧 **Cada dispositivo con lo suyo**: cada salida recuerda su EQ, su Sonido, sus Cascos y el último preset de cada sección, y cada micrófono lo suyo. Al cambiar de salida (a mano o siguiendo a Windows) suena y se ve lo de esa salida.
- 🔗 **Compartir presets**: de cualquier sección, como código para pegar en un chat o como archivo `.poxieq`; al importarlo va a su sección.
- 📈 **Analizador mejorado**: retención de picos y modo espectrograma.
- 🎧 **Altavoces virtuales** para cascos: el estéreo suena delante de ti, como con dos altavoces, sin cambiar el timbre del centro.
- 🎛️ **Estudio**: mono, side, solo L / R, invertir polaridad e intercambiar canales, y simulación de móvil, portátil, coche, auriculares de botón, tele y altavoz bluetooth.
- 👂 **Test de audición**: crea el preset «Mi audición» con una compensación suave para cada oído.
- 📐 **Corrección de sala**: mide tus altavoces con un micrófono en varias posiciones y crea un preset que corta las resonancias de graves.
- 🎙️ **Micrófono**: EQ, puerta de ruido y compresor antes de que llegue a Discord, OBS o cualquier app, con medidor de entrada.

### 🇬🇧 English

- 🎚️ **Equalizer for all of Windows**: games, music, calls and browser, with zero latency.
- 🧭 **Organized by task**: side menu with Equalizer, Sound, Headphones, Studio, Room and Microphone. Nothing hidden.
- 🔁 **Follows the Windows output**: plug in headphones or a Bluetooth speaker and PoxiEQ switches to its EQ.
- ⚡ **One-click setup, no reboot**: PoxiEQ installs its engine and enables it on your headphones or speakers.
- 🖱️ **Live curve**: drag the points, the wheel changes the width and the sound changes instantly.
- 🎧 **Find my headphones**: type your model and apply its AutoEq correction.
- 🛡️ **Automatic preamp** so it never clips, **bypass** with `Ctrl+Alt+Num 0` and from the tray.
- 🔊 **Dynamics**: loudness presets (Natural, Streaming, Loud, Night), true-peak limiter, compressor, LUFS leveler, bass harmonics, dialogue enhancer and crossfeed.
- 🔈 **Bass on a knob**: up to +10 dB of clean, punchy low end (no muddy mids, no distortion) with protection so it never clips.
- 🎯 **Preset per app**: each app with its own preset (Spotify → Music, a game → Footsteps) and yours comes back when you leave.
- ⌨️ **Your own shortcuts**: record them in Settings (by default `Ctrl+Alt+Num 0…9`), with an on-screen notice that doesn't steal focus. Combinations that clash with AltGr (`€ | @ # ~`) aren't allowed, and you're warned if another program already owns the shortcut.
- 🗂️ **Presets in the tray**: switch presets without opening the window.
- ↩️ **Undo and redo**: `Ctrl+Z` and `Ctrl+Y` for the curve and dynamics.
- 💾 **Presets in every section**: save and switch your Sound, Headphones listening, Studio checks and Microphone settings with one click, just like Equalizer curves.
- 🎧 **Each device keeps its own**: every output remembers its EQ, Sound, Headphones settings and the last preset of each section, and every microphone its own. Switch outputs (by hand or following Windows) and you hear and see that output's settings.
- 🔗 **Share presets**: from any section, as a code to paste in a chat or as a `.poxieq` file; importing it puts it in the right section.
- 📈 **Better analyzer**: peak hold and a spectrogram mode.
- 🎧 **Virtual speakers** for headphones: stereo plays in front of you, like two speakers, without changing the tone of the center.
- 🎛️ **Studio**: mono, side, solo L / R, polarity invert and channel swap, plus phone, laptop, car, earbuds, TV and Bluetooth speaker simulation.
- 👂 **Hearing test**: creates the "My hearing" preset with a gentle compensation for each ear.
- 📐 **Room correction**: measure your speakers with a microphone at several positions and create a preset that cuts the bass resonances.
- 🎙️ **Microphone**: EQ, noise gate and compressor before it reaches Discord, OBS or any app, with an input meter.
