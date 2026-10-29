/**
 * Development-only stand-in for the Tauri IPC, so Hub screens render in a plain browser for
 * screenshots and UI review (`?page=history`, `?section=transcription`, `?onboarding=1&step=2`).
 * Loaded only by `import.meta.env.DEV` builds when `window.__TAURI_INTERNALS__` is missing.
 * Sample data only; nothing here talks to a backend.
 */
const params = new URLSearchParams(location.search);
const now = Date.now();
const MIN = 60_000;

const settings = {
  version: 1,
  hotkey: "Ctrl+Super",
  double_tap_window_ms: 300,
  max_session_minutes: 5,
  microphone: "Microphone (HyperX SoloCast)",
  active_model: "whisper-large-v3-turbo",
  use_gpu: true,
  language: "auto",
  unload_after_idle_minutes: 30,
  cleanup: "ollama",
  cleanup_model: "",
  cleanup_base_url: "",
  cleanup_timeout_seconds: 15,
  cleanup_style: (params.get("style") as string) ?? "standard",
  cleanup_custom_prompt: "",
  vocabulary: ["Kubernetes", "PostgreSQL", "Siobhan", "Córdoba"],
  replacements: [
    { from: "post gres", to: "PostgreSQL" },
    { from: "kay eight s", to: "k8s" },
  ],
  insert_method: "paste",
  restore_clipboard: false,
  show_idle_bubble: true,
  sounds_enabled: true,
  sound_volume: 0.6,
  history_retention_days: 30,
  keep_audio: false,
  ui_language: (params.get("lang") as string) ?? "system",
  onboarding_completed: params.get("onboarding") !== "1",
};

const texts = [
  "Hey Priya, can you push the Kubernetes config to staging before Thursday?",
  "Mañana a las diez tenemos la reunión con el equipo de Córdoba.",
  "Let's roll back to version 2.7 and ping Siobhan about the Grafana dashboard.",
  "Sounds good",
  "Acordate de llevar el informe trimestral y revisá el presupuesto antes del jueves.",
  "We should meet on Wednesday at three and bring the quarterly numbers.",
];
const history = texts.map((text, i) => ({
  id: texts.length - i,
  createdAt: now - (i < 3 ? i * 17 * MIN + 4 * MIN : 26 * 60 * MIN + i * 40 * MIN),
  text,
  raw: `um ${text.toLowerCase()}`,
  engine: "whisper-large-v3-turbo",
  durationMs: 3000 + i * 1700,
  latencyMs: 300,
  words: text.split(" ").length,
  targetApp: ["notepad", "code", "chrome", "slack", "windowsterminal", "notepad"][i],
}));

const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
  get_settings: () => settings,
  update_settings: (a) => a.settings,
  list_microphones: () => [
    { name: "Microphone (HyperX SoloCast)", isDefault: false },
    { name: "Micrófono (2- H510-PRO Wireless headset)", isDefault: true },
  ],
  start_mic_preview: () => null,
  stop_mic_preview: () => null,
  get_engine_status: () => ({
    state: "ready",
    model: "whisper-large-v3-turbo",
    onGpu: true,
    device: "RTX 5060 Ti",
    gpuFallback: false,
  }),
  list_models: () => [
    {
      id: "whisper-large-v3-turbo",
      name: "Whisper large-v3-turbo",
      sizeBytes: 619628096,
      multilingual: true,
      downloaded: params.get("onboarding") !== "1",
      downloading: false,
      active: true,
    },
    {
      id: "parakeet-tdt-0.6b-v3",
      name: "Parakeet TDT 0.6B v3",
      sizeBytes: 548946272,
      multilingual: true,
      downloaded: false,
      downloading: false,
      active: false,
    },
    {
      id: "whisper-base.en",
      name: "Whisper base.en",
      sizeBytes: 63709472,
      multilingual: false,
      downloaded: true,
      downloading: false,
      active: false,
    },
  ],
  get_history: (a) =>
    history.filter((h) => !a.query || h.text.toLowerCase().includes(String(a.query).toLowerCase())),
  get_stats: () => ({
    wordsThisWeek: 1842,
    averageWpm: 148,
    minutesSaved: 33.6,
    dictationsThisWeek: 57,
  }),
  get_last_timings: () => ({
    hotkeyToRecordMs: 24.6,
    releaseToStopMs: 1.2,
    audioMs: 5120,
    vadMs: 9,
    transcribeMs: 412,
    cleanupMs: 640,
    insertMs: 96,
    totalMs: 1171,
    engine: "whisper-large-v3-turbo",
  }),
  get_autostart: () => true,
  // `?perm=denied` / `?perm=notAsked` and `?platform=windows` preview the permission states.
  get_permissions: () => {
    const perm = params.get("perm") ?? "granted";
    const windows = params.get("platform") === "windows";
    return {
      platform: windows ? "windows" : "macos",
      required: true,
      microphone: perm,
      microphoneBlock: windows && perm !== "granted" ? "desktopApps" : null,
      hasMicrophone: params.get("nomic") !== "1",
      accessibility: windows ? "granted" : perm === "granted" ? "granted" : "denied",
      inputMonitoring: "granted",
      hotkeysActive: windows || perm === "granted",
    };
  },
  request_permission: () => "needsSettings",
  reset_permission: () => "needsSettings",
  "plugin:app|version": () => "0.1.0",
  "plugin:event|listen": () => Math.floor(Math.random() * 1e6),
  "plugin:event|unlisten": () => null,
};

let callbackId = 0;
Object.assign(window, {
  __TAURI_INTERNALS__: {
    invoke: async (cmd: string, args: Record<string, unknown> = {}) =>
      (handlers[cmd] ?? (() => null))(args),
    transformCallback: () => ++callbackId,
    unregisterCallback: () => {},
    convertFileSrc: (p: string) => p,
    metadata: {
      currentWindow: { label: "hub" },
      currentWebview: { windowLabel: "hub", label: "hub" },
    },
  },
  __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
});

export {};
