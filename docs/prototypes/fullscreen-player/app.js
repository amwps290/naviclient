"use strict";

const DESIGNS = {
  cinema: { number: "01", name: "暮色剧场", mood: "把房间交给音乐", description: "沉浸氛围，大屏聆听", visual: "wave" },
  gallery: { number: "02", name: "白昼画廊", mood: "让音乐，留一点白", description: "暖白留白，安静而清晰", visual: "bars" },
  stage: { number: "03", name: "夜航现场", mood: "每一拍，都看得见", description: "节拍可见，现场演示", visual: "bars" },
  vinyl: { number: "04", name: "黑胶沙龙", mood: "一张唱片，一整个夜晚", description: "唱片旋转，慢慢聆听", visual: "wave" },
};
const DEFAULT_DESIGN = "vinyl";

const SONGS = [
  {
    title: "蓝色时刻", artist: "林间回声", album: "The Blue Hour", cover: "art/blue-hour.svg", duration: 258,
    alt: "蓝色时刻：暮色海面与一轮柔和的落日",
    lyrics: [
      [0, "城市把灯一盏盏亮起", "The city lights up, one window at a time."],
      [12, "落日还没说完的梦", "The sunset has a dream left to tell."],
      [25, "沿着海岸轻轻落下", "It settles softly along the coast."],
      [38, "你的影子经过我的窗", "Your shadow passes by my window."],
      [50, "夜色正温柔地落下", "The night falls softly."],
      [62, "你是远处唯一的光", "You are the only light in the distance."],
      [74, "让晚风替我回答", "Let the evening breeze speak for me."],
      [86, "也让海浪慢慢停下", "Let the waves come gently to rest."],
      [98, "在白昼和夜晚之间", "Somewhere between the day and night."],
      [112, "我们把时间轻轻放下", "We put time down, ever so gently."],
      [126, "没有说完的那句话", "The words we never finished saying."],
      [140, "留给明天的第一道光", "We leave them to the first light of tomorrow."],
      [154, "让晚风替我回答", "Let the evening breeze speak for me."],
      [168, "也让海浪慢慢停下", "Let the waves come gently to rest."],
      [182, "就这样再靠近一点", "Stay just a little closer."],
      [196, "听见心里安静的声响", "Hear the quiet sound within."],
      [210, "所有远方都有回声", "Every distant place has an echo."],
      [224, "所有夜晚终会天亮", "Every night gives way to dawn."],
      [238, "而你就在这片光里", "And there you are, within this light."],
      [250, "陪我等下一次日落", "Waiting with me for another sunset."],
    ],
  },
  {
    title: "海岸来信", artist: "林间回声", album: "Quiet Tides", cover: "art/quiet-tides.svg", duration: 244,
    alt: "海岸来信：青色海浪、浅色沙滩和一轮白日",
    lyrics: [
      [0, "海岸寄来一封信", "A letter arrives from the coast."],
      [15, "写着潮汐和你的名字", "With the tide and your name inside."],
      [29, "我把它放进口袋", "I keep it in my pocket."],
      [43, "陪我走过有风的日子", "Through all these windy days."],
      [58, "远方不是一个方向", "The distance is not a direction."],
      [72, "是有人等你回去的地方", "It is where someone waits for you."],
      [87, "听海替你轻轻说", "Listen to the sea speak softly for you."],
      [101, "别着急，天还很长", "Take your time. There is daylight left."],
      [116, "那些没有落下的雨", "The rain that never fell."],
      [130, "都变成温柔的海浪", "Has become these gentle waves."],
      [145, "我沿着你的笔迹", "I follow the shape of your words."],
      [159, "走回最初的那片沙滩", "Back to the beach where we began."],
      [174, "听海替你轻轻说", "Listen to the sea speak softly for you."],
      [188, "别着急，天还很长", "Take your time. There is daylight left."],
      [203, "等下一次潮水退去", "When the tide goes out again."],
      [217, "我会把答案写给你", "I will write my answer to you."],
      [232, "就在有光的地方", "Right here, where the light is."],
    ],
  },
  {
    title: "慢慢天亮", artist: "林间回声", album: "Slow Morning", cover: "art/slow-morning.svg", duration: 232,
    alt: "慢慢天亮：柔和的朝阳、绿色远山和一条蜿蜒的小路",
    lyrics: [
      [0, "昨夜的雨已经停下", "Last night's rain has passed."],
      [14, "阳光经过你的肩膀", "The sunlight rests upon your shoulder."],
      [28, "没有一定要去的地方", "There is nowhere we have to be."],
      [42, "就沿着这条小路走吧", "Let's take this little road."],
      [56, "我们不必赶上时间", "We do not have to catch up with time."],
      [70, "也不必追问每个答案", "Or find an answer to everything."],
      [84, "让世界慢慢天亮", "Let the world slowly wake to light."],
      [98, "让心事慢慢变得柔软", "Let our worries gently soften."],
      [112, "一片叶子落在手上", "A leaf comes to rest in my hand."],
      [126, "像一封来自春天的信", "Like a letter sent by spring."],
      [140, "你笑着把它收起来", "You smile and put it away."],
      [154, "说今天会是很好的一天", "You say this will be a lovely day."],
      [168, "让世界慢慢天亮", "Let the world slowly wake to light."],
      [182, "让心事慢慢变得柔软", "Let our worries gently soften."],
      [196, "山的那边还有微风", "There is a breeze beyond the hill."],
      [210, "我们就在这里停一停", "Let's stop right here for a while."],
      [224, "等下一朵云经过", "And wait for the next cloud to pass."],
    ],
  },
];

const $ = (id) => document.getElementById(id);
const params = new URLSearchParams(location.search);
const initialTime = Number(params.get("t") ?? 84);
const state = {
  design: Object.hasOwn(DESIGNS, params.get("design")) ? params.get("design") : DEFAULT_DESIGN,
  song: 0,
  playing: params.get("paused") !== "1",
  base: Number.isFinite(initialTime) ? Math.max(0, Math.min(258, initialTime)) : 84,
  started: performance.now(),
  muted: true,
  volume: 45,
  shuffle: false,
  repeat: false,
  translated: true,
  visual: "wave",
  scrubbing: false,
  activeLine: -1,
  freeze: params.get("freeze") === "1",
};

let lyricElements = [];
let toastTimer;
let idleTimer;
let lastPaint = 0;
let lastProgressPaint = 0;
let canvasWidth = 0;
let canvasHeight = 0;
let visualColor = "#efb5a5";
const canvas = $("visualizer");
const painter = canvas.getContext("2d");
const levels = new Float32Array(64);
const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");

function icon(button, name) {
  button.querySelector("use").setAttribute("href", `#i-${name}`);
}

function toast(message) {
  clearTimeout(toastTimer);
  $("toast").textContent = message;
  $("toast").classList.add("visible");
  toastTimer = setTimeout(() => $("toast").classList.remove("visible"), 2500);
}

function formatTime(seconds) {
  const whole = Math.max(0, Math.floor(seconds));
  return `${String(Math.floor(whole / 60)).padStart(2, "0")}:${String(whole % 60).padStart(2, "0")}`;
}

function position(now = performance.now()) {
  return state.base + (state.playing && !state.scrubbing && !state.freeze ? (now - state.started) / 1000 : 0);
}

function seek(seconds) {
  state.base = Math.max(0, Math.min(SONGS[state.song].duration, seconds));
  state.started = performance.now();
  updateProgress(true);
}

function setDesign(design, updateAddress = true) {
  if (!Object.hasOwn(DESIGNS, design)) return;
  state.design = design;
  state.visual = DESIGNS[design].visual;
  document.body.dataset.design = design;
  document.querySelectorAll(".design-option").forEach((button) => {
    button.setAttribute("aria-pressed", String(button.dataset.design === design));
  });
  $("design-mood").textContent = DESIGNS[design].mood;
  $("design-description").replaceChildren(
    document.createTextNode(`${DESIGNS[design].number} / ${DESIGNS[design].name}`),
    Object.assign(document.createElement("span"), { className: "caption-dot", textContent: "·" }),
    document.createTextNode(DESIGNS[design].description),
  );
  if (updateAddress) {
    const address = new URL(location.href);
    address.searchParams.set("design", design);
    history.replaceState(null, "", address);
  }
  $("player").classList.remove("no-visualizer");
  updateVisualizerLabel();
  requestAnimationFrame(() => {
    visualColor = getComputedStyle(document.body).getPropertyValue("--accent").trim();
    resizeCanvas();
    centerLyrics();
  });
}

function renderSong(index, start = 0) {
  state.song = (index + SONGS.length) % SONGS.length;
  state.activeLine = -1;
  const song = SONGS[state.song];
  $("song-title").textContent = song.title;
  $("artist").textContent = song.artist;
  $("album-name").textContent = song.album;
  $("album-kicker").textContent = song.album.toUpperCase();
  $("cover").src = song.cover;
  $("cover").alt = song.alt;
  document.body.style.setProperty("--art-url", `url("${song.cover}")`);
  $("seek").max = String(song.duration);
  $("duration").textContent = formatTime(song.duration);
  $("favorite").setAttribute("aria-pressed", String(favorites.has(state.song)));
  $("favorite").setAttribute("aria-label", favorites.has(state.song) ? "取消收藏这首歌曲" : "收藏这首歌曲");
  const fragment = document.createDocumentFragment();
  lyricElements = song.lyrics.map(([time, text, translation], index) => {
    const button = document.createElement("button");
    button.className = "lyric-line";
    button.dataset.index = String(index);
    button.setAttribute("aria-label", `跳转到 ${formatTime(time)}：${text}`);
    const original = document.createElement("span");
    original.className = "lyric-text";
    original.textContent = text;
    const secondary = document.createElement("span");
    secondary.className = "translation";
    secondary.textContent = translation;
    button.append(original, secondary);
    button.addEventListener("click", () => { seek(time); toast(`回到「${text}」`); });
    fragment.append(button);
    return button;
  });
  $("lyrics-track").replaceChildren(fragment);
  seek(start);
  updateAudio();
}

function centerLyrics() {
  const current = lyricElements[state.activeLine];
  if (!current) return;
  const offset = $("lyrics-viewport").clientHeight / 2 - current.offsetTop - current.offsetHeight / 2;
  $("lyrics-track").style.transform = `translateY(${offset}px)`;
}

function updateProgress(force = false) {
  const now = performance.now();
  const song = SONGS[state.song];
  let time = Math.min(song.duration, position(now));
  if (time >= song.duration && state.playing && !state.scrubbing) {
    if (state.repeat) seek(0);
    else nextSong(1, false);
    return;
  }
  if (force || now - lastProgressPaint > 180) {
    $("elapsed").textContent = formatTime(time);
    if (!state.scrubbing) $("seek").value = String(time);
    $("seek").style.setProperty("--progress", `${time / song.duration * 100}%`);
    $("seek").setAttribute("aria-valuetext", `${formatTime(time)}，总长 ${formatTime(song.duration)}`);
    lastProgressPaint = now;
  }
  let active = 0;
  for (let index = 0; index < song.lyrics.length; index++) {
    if (song.lyrics[index][0] <= time) active = index;
  }
  if (active !== state.activeLine || force) {
    state.activeLine = active;
    lyricElements.forEach((line, index) => {
      line.classList.toggle("active", index === active);
      line.classList.toggle("distant", Math.abs(index - active) > 2);
      line.setAttribute("aria-current", String(index === active));
    });
    $("lyric-position").textContent = `${String(active + 1).padStart(2, "0")} / ${song.lyrics.length}`;
    centerLyrics();
    // The active translation changes row height during its short transition.
    requestAnimationFrame(centerLyrics);
  }
  const start = song.lyrics[active][0];
  const end = song.lyrics[active + 1]?.[0] ?? song.duration;
  const fill = Math.max(0, Math.min(100, (time - start) / Math.max(1, end - start) * 100));
  lyricElements[active]?.style.setProperty("--fill", `${fill}%`);
}

function setPlaying(playing) {
  state.base = Math.min(SONGS[state.song].duration, position());
  state.started = performance.now();
  state.playing = playing;
  $("player").classList.toggle("paused", !playing);
  icon($("play"), playing ? "pause" : "play");
  $("play").setAttribute("aria-label", playing ? "暂停" : "播放");
  $("playback-label").textContent = playing ? "正在播放" : "已暂停";
  updateAudio();
}

function nextSong(direction, announce = true) {
  const index = state.shuffle && direction > 0
    ? (state.song + 1 + Math.floor(Math.random() * (SONGS.length - 1))) % SONGS.length
    : state.song + direction;
  renderSong(index);
  if (announce) toast(`正在聆听 · ${SONGS[state.song].title}`);
}

function updateVisualizerLabel() {
  const name = state.visual === "wave" ? "声音的形状" : state.visual === "bars" ? "每一拍的回声" : "静静聆听";
  $("visualizer-label").textContent = name;
  $("visualizer-caption").textContent = state.visual === "bars" ? "RHYTHM IN VIEW" : "SOUND IN MOTION";
  $("visualizer-button").setAttribute("aria-label", `音频可视化：${state.visual === "wave" ? "波形" : state.visual === "bars" ? "频谱" : "关闭"}，点击切换`);
  $("player").classList.toggle("no-visualizer", state.visual === "off");
}

function toggleVisualizer() {
  const modes = ["wave", "bars", "off"];
  state.visual = modes[(modes.indexOf(state.visual) + 1) % modes.length];
  updateVisualizerLabel();
  toast(state.visual === "wave" ? "波形视效" : state.visual === "bars" ? "频谱视效" : "动态视效已关闭");
}

function toggleTranslation() {
  state.translated = !state.translated;
  $("player").classList.toggle("no-translation", !state.translated);
  $("translation-button").setAttribute("aria-pressed", String(state.translated));
  $("translation-button").setAttribute("aria-label", state.translated ? "隐藏歌词译文" : "显示歌词译文");
  centerLyrics();
}

function resizeCanvas() {
  const bounds = canvas.getBoundingClientRect();
  canvasWidth = bounds.width;
  canvasHeight = bounds.height;
  const scale = Math.min(devicePixelRatio || 1, 2);
  canvas.width = Math.round(canvasWidth * scale);
  canvas.height = Math.round(canvasHeight * scale);
  painter.setTransform(scale, 0, 0, scale, 0, 0);
}

function paintVisualizer(now) {
  if (!canvasWidth || !canvasHeight || state.visual === "off") return;
  const time = now / 1000;
  const real = demoAudio.readSpectrum();
  const moving = state.playing && !reducedMotion.matches;
  for (let index = 0; index < levels.length; index++) {
    const shape = .18 + .65 * Math.pow(Math.sin((index / levels.length) * Math.PI), .7);
    const pulse = .27 + .3 * Math.sin(time * 2.2 + index * .24) ** 2 + .26 * Math.sin(time * 4.4 - index * .12) ** 2;
    const target = moving ? (real ? real[index] : shape * pulse) : .025;
    levels[index] += (target - levels[index]) * .17;
  }
  painter.clearRect(0, 0, canvasWidth, canvasHeight);
  if (state.visual === "bars") {
    const gap = state.design === "stage" ? 3.5 : 3;
    const step = canvasWidth / levels.length;
    const width = Math.max(1, step - gap);
    painter.fillStyle = visualColor;
    for (let index = 0; index < levels.length; index++) {
      const height = Math.max(2, levels[index] * (canvasHeight - 5));
      const y = state.design === "gallery" ? (canvasHeight - height) / 2 : canvasHeight - height - 2;
      painter.globalAlpha = .35 + levels[index] * .65;
      painter.beginPath();
      painter.roundRect(index * step, y, width, height, Math.min(1.5, width / 2));
      painter.fill();
    }
  } else {
    const average = levels.reduce((sum, level) => sum + level, 0) / levels.length;
    for (let layer = 0; layer < 3; layer++) {
      painter.beginPath();
      painter.strokeStyle = visualColor;
      painter.lineWidth = layer === 0 ? 1.3 : .8;
      painter.globalAlpha = layer === 0 ? .9 : .22;
      for (let x = 0; x <= canvasWidth; x += 2) {
        const fraction = x / canvasWidth;
        const taper = Math.sin(fraction * Math.PI) ** 1.4;
        const phase = fraction * Math.PI * (6 + layer * 2) - time * (1.1 + layer * .3);
        const height = taper * (average * .8 + .06) * canvasHeight * .54;
        const y = canvasHeight / 2 + Math.sin(phase) * height + Math.sin(phase * 1.7 + time) * height * .24;
        if (x === 0) painter.moveTo(x, y);
        else painter.lineTo(x, y);
      }
      painter.stroke();
    }
  }
  painter.globalAlpha = 1;
}

// A small, original instrumental soundscape. It only starts after a user
// explicitly enables audition; there are no audio files or external services.
class DemoAudio {
  constructor() {
    this.context = null;
    this.timer = null;
    this.nextBeat = 0;
    this.beat = 0;
    this.chords = [[57, 60, 64, 71], [53, 57, 60, 64], [48, 55, 59, 64], [55, 59, 62, 69]];
    this.spectrum = new Float32Array(64);
  }

  create() {
    const Audio = window.AudioContext || window.webkitAudioContext;
    if (!Audio) throw new Error("这个浏览器暂不支持试听。");
    this.context = new Audio();
    this.bus = this.context.createGain();
    this.bus.gain.value = .68;
    const compressor = this.context.createDynamicsCompressor();
    compressor.threshold.value = -22;
    compressor.ratio.value = 3;
    this.analyser = this.context.createAnalyser();
    this.analyser.fftSize = 2048;
    this.analyser.smoothingTimeConstant = .82;
    this.output = this.context.createGain();
    this.output.gain.value = 0;
    this.bus.connect(compressor);
    compressor.connect(this.analyser);
    this.analyser.connect(this.output);
    this.output.connect(this.context.destination);
    this.frequencyBytes = new Uint8Array(this.analyser.frequencyBinCount);
    this.noise = this.context.createBuffer(1, this.context.sampleRate * .12, this.context.sampleRate);
    const samples = this.noise.getChannelData(0);
    for (let index = 0; index < samples.length; index++) samples[index] = Math.random() * 2 - 1;
  }

  async audition(volume, playing) {
    if (!this.context) this.create();
    this.setVolume(volume);
    await this.setPlaying(playing);
  }

  setVolume(volume) {
    if (!this.context) return;
    this.output.gain.setTargetAtTime((volume / 100) ** 1.6 * .65, this.context.currentTime, .07);
  }

  async setPlaying(playing) {
    if (!this.context) return;
    clearInterval(this.timer);
    this.timer = null;
    if (!playing) { await this.context.suspend(); return; }
    await this.context.resume();
    this.nextBeat = this.context.currentTime + .07;
    this.schedule();
    if (!this.timer) this.timer = setInterval(() => this.schedule(), 100);
  }

  note(midi, at, length, level, type = "sine") {
    const oscillator = this.context.createOscillator();
    const envelope = this.context.createGain();
    oscillator.type = type;
    oscillator.frequency.value = 440 * 2 ** ((midi - 69) / 12);
    envelope.gain.setValueAtTime(.0001, at);
    envelope.gain.exponentialRampToValueAtTime(level, at + .04);
    envelope.gain.exponentialRampToValueAtTime(.0001, at + length);
    oscillator.connect(envelope);
    envelope.connect(this.bus);
    oscillator.start(at);
    oscillator.stop(at + length + .03);
    oscillator.onended = () => { oscillator.disconnect(); envelope.disconnect(); };
  }

  schedule() {
    const beatLength = 60 / 76;
    this.nextBeat = Math.max(this.nextBeat, this.context.currentTime + .025);
    while (this.nextBeat < this.context.currentTime + .22) {
      const chord = this.chords[Math.floor(this.beat / 8) % this.chords.length];
      if (this.beat % 8 === 0) {
        chord.forEach((note, index) => this.note(note, this.nextBeat + index * .035, 6.3, .10, "triangle"));
      }
      if (this.beat % 2 === 0) {
        this.note(chord[0] - 12, this.nextBeat, .75, .17);
        this.note(chord[(this.beat / 2) % chord.length] + 12, this.nextBeat + .06, 1.45, .035);
      }
      const hat = this.context.createBufferSource();
      const filter = this.context.createBiquadFilter();
      const envelope = this.context.createGain();
      hat.buffer = this.noise;
      filter.type = "highpass";
      filter.frequency.value = 5700;
      envelope.gain.setValueAtTime(.025, this.nextBeat + beatLength * .5);
      envelope.gain.exponentialRampToValueAtTime(.0001, this.nextBeat + beatLength * .5 + .08);
      hat.connect(filter); filter.connect(envelope); envelope.connect(this.bus);
      hat.start(this.nextBeat + beatLength * .5);
      hat.onended = () => { hat.disconnect(); filter.disconnect(); envelope.disconnect(); };
      this.nextBeat += beatLength;
      this.beat++;
    }
  }

  readSpectrum() {
    if (!this.context || this.context.state !== "running" || state.muted) return null;
    this.analyser.getByteFrequencyData(this.frequencyBytes);
    for (let index = 0; index < this.spectrum.length; index++) {
      const frequency = 45 * (6500 / 45) ** (index / (this.spectrum.length - 1));
      const bin = Math.min(this.frequencyBytes.length - 1, Math.floor(frequency / (this.context.sampleRate / 2) * this.frequencyBytes.length));
      this.spectrum[index] = this.frequencyBytes[bin] / 255;
    }
    return this.spectrum;
  }
}

const favorites = new Set();
const demoAudio = new DemoAudio();

function updateAudio() {
  demoAudio.setPlaying(state.playing && !state.muted).catch(() => toast("点击喇叭重新开启试听。"));
}

async function setMuted(muted) {
  state.muted = muted;
  icon($("mute"), muted ? "muted" : "volume");
  $("mute").setAttribute("aria-label", muted ? "开启示例音轨试听" : "关闭示例音轨试听");
  $("mute").title = muted ? "开启示例音轨试听" : "静音";
  try {
    if (muted) await demoAudio.setPlaying(false);
    else await demoAudio.audition(state.volume, state.playing);
  } catch (error) {
    state.muted = true;
    icon($("mute"), "muted");
    $("mute").setAttribute("aria-label", "开启示例音轨试听");
    toast(error.message || "试听暂时无法开启。");
  }
}

function wakeControls() {
  clearTimeout(idleTimer);
  $("player").classList.remove("chrome-idle");
  if (document.fullscreenElement && !$("design-info").open && !$("player").classList.contains("chrome-hidden")) {
    idleTimer = setTimeout(() => {
      if (document.activeElement?.matches("input, button, a") || document.querySelector(".chrome:hover")) return;
      $("player").classList.add("chrome-idle");
    }, 3200);
  }
}

function hideControls() {
  const hidden = $("player").classList.toggle("chrome-hidden");
  $("hide-controls").setAttribute("aria-label", hidden ? "显示控件" : "隐藏控件");
  if (hidden) {
    document.activeElement?.blur();
    toast("纯净展示 · 按 H 唤回控件");
  }
  wakeControls();
}

async function toggleFullscreen() {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await document.documentElement.requestFullscreen();
  } catch {
    toast("浏览器未允许全屏，可以使用 F11。");
  }
}

document.querySelectorAll(".design-option, .design-note").forEach((button) => {
  button.addEventListener("click", () => {
    setDesign(button.dataset.design);
    if ($("design-info").open) $("design-info").close();
    wakeControls();
  });
});
$("play").addEventListener("click", () => setPlaying(!state.playing));
$("next").addEventListener("click", () => nextSong(1));
$("previous").addEventListener("click", () => nextSong(-1));
$("seek").addEventListener("input", (event) => seek(Number(event.target.value)));
$("seek").addEventListener("pointerdown", () => { state.base = position(); state.scrubbing = true; });
function finishScrubbing() {
  if (!state.scrubbing) return;
  state.scrubbing = false;
  state.started = performance.now();
}
window.addEventListener("pointerup", finishScrubbing);
window.addEventListener("pointercancel", finishScrubbing);
$("mute").addEventListener("click", () => { const muted = !state.muted; setMuted(muted); toast(muted ? "试听已关闭" : "试听已开启 · 原创氛围音轨"); });
$("volume").addEventListener("input", (event) => {
  state.volume = Number(event.target.value);
  $("volume").style.setProperty("--progress", `${state.volume}%`);
  demoAudio.setVolume(state.volume);
  if (state.volume === 0) setMuted(true);
  else if (state.muted) setMuted(false);
});
$("shuffle").addEventListener("click", () => {
  state.shuffle = !state.shuffle;
  $("shuffle").setAttribute("aria-pressed", String(state.shuffle));
  $("shuffle").setAttribute("aria-label", state.shuffle ? "关闭随机播放" : "开启随机播放");
  toast(state.shuffle ? "随机播放已开启" : "按顺序播放");
});
$("repeat").addEventListener("click", () => {
  state.repeat = !state.repeat;
  $("repeat").setAttribute("aria-pressed", String(state.repeat));
  $("repeat").setAttribute("aria-label", state.repeat ? "关闭单曲循环" : "开启单曲循环");
  toast(state.repeat ? "单曲循环已开启" : "单曲循环已关闭");
});
$("favorite").addEventListener("click", () => {
  if (favorites.has(state.song)) favorites.delete(state.song);
  else favorites.add(state.song);
  const selected = favorites.has(state.song);
  $("favorite").setAttribute("aria-pressed", String(selected));
  $("favorite").setAttribute("aria-label", selected ? "取消收藏这首歌曲" : "收藏这首歌曲");
  toast(selected ? "已收藏这首歌曲" : "已取消收藏");
});
$("visualizer-button").addEventListener("click", toggleVisualizer);
$("translation-button").addEventListener("click", toggleTranslation);
$("fullscreen-button").addEventListener("click", toggleFullscreen);
$("hide-controls").addEventListener("click", hideControls);
$("info-button").addEventListener("click", () => { $("design-info").showModal(); wakeControls(); });
$("close-info").addEventListener("click", () => $("design-info").close());
$("design-info").addEventListener("click", (event) => {
  const bounds = $("design-info").getBoundingClientRect();
  if (event.target === $("design-info") && (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom)) $("design-info").close();
});
$("design-info").addEventListener("close", wakeControls);
document.addEventListener("fullscreenchange", () => {
  const fullscreen = Boolean(document.fullscreenElement);
  icon($("fullscreen-button"), fullscreen ? "collapse" : "expand");
  $("fullscreen-button").querySelector("span").textContent = fullscreen ? "退出" : "全屏";
  $("fullscreen-button").setAttribute("aria-label", fullscreen ? "退出全屏" : "进入全屏");
  $("fullscreen-button").blur();
  if (!fullscreen) $("player").classList.remove("chrome-hidden", "chrome-idle");
  wakeControls();
});
document.addEventListener("pointermove", wakeControls, { passive: true });
document.addEventListener("keydown", (event) => {
  if (event.ctrlKey || event.altKey || event.metaKey || event.target.matches("input, textarea") || $("design-info").open) return;
  wakeControls();
  const key = event.key.toLowerCase();
  if (["1", "2", "3", "4"].includes(key)) { setDesign(Object.keys(DESIGNS)[Number(key) - 1]); event.preventDefault(); }
  else if (key === " " && !event.target.closest("button, a")) { setPlaying(!state.playing); event.preventDefault(); }
  else if (key === "f") { toggleFullscreen(); event.preventDefault(); }
  else if (key === "h") { hideControls(); event.preventDefault(); }
  else if (key === "t") { toggleTranslation(); event.preventDefault(); }
  else if (key === "v") { toggleVisualizer(); event.preventDefault(); }
  else if (key === "arrowright" || key === "arrowleft") { seek(position() + (key === "arrowright" ? 5 : -5)); event.preventDefault(); }
  else if (key === "escape" && $("player").classList.contains("chrome-hidden")) { hideControls(); event.preventDefault(); }
});

new ResizeObserver(() => { resizeCanvas(); centerLyrics(); }).observe($("lyrics-viewport"));
new ResizeObserver(resizeCanvas).observe(canvas);
$("lyrics-track").addEventListener("transitionend", (event) => { if (event.propertyName === "max-height") centerLyrics(); });
window.addEventListener("resize", () => requestAnimationFrame(centerLyrics), { passive: true });
$("artwork-wrap").addEventListener("pointermove", (event) => {
  if (state.design !== "cinema" || reducedMotion.matches) return;
  const bounds = $("artwork-wrap").getBoundingClientRect();
  $("artwork-wrap").style.setProperty("--tilt-x", `${((event.clientX - bounds.left) / bounds.width - .5) * 4}deg`);
  $("artwork-wrap").style.setProperty("--tilt-y", `${-((event.clientY - bounds.top) / bounds.height - .5) * 4}deg`);
});
$("artwork-wrap").addEventListener("pointerleave", () => {
  $("artwork-wrap").style.setProperty("--tilt-x", "0deg");
  $("artwork-wrap").style.setProperty("--tilt-y", "0deg");
});

function frame(now) {
  if (now - lastPaint > 1000 / 30) {
    updateProgress();
    paintVisualizer(now);
    lastPaint = now;
  }
  requestAnimationFrame(frame);
}

setDesign(state.design, false);
renderSong(0, state.base);
setPlaying(state.playing);
$("volume").style.setProperty("--progress", `${state.volume}%`);
requestAnimationFrame(frame);
