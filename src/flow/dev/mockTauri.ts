import type {
  AppSettings,
  AudioDevice,
  HistoryEntry,
  ModelInfo,
} from "@/bindings";
import { APP_NAME } from "@/flow/brand";

// Fake Tauri runtime for ?flowPreview=1 in a plain browser. Answers the
// commands the shell and pages call with neutral sample data; events never fire.
// Page-specific answers live in ./mocks/*.ts, each exporting
// `handlers: Record<string, MockHandler>`; they override the defaults below.

export type MockHandler = (args: Record<string, unknown>) => unknown;

const SAMPLE_TEXTS = [
  "Let's move the weekly sync to Thursday afternoon so the design review has time to wrap up first. I'll send a new invite once everyone confirms.",
  "Quick reminder to update the onboarding checklist before Friday. The second section still points to the old setup guide.",
  "Can you share the latest numbers from the pilot? I want to compare them with last quarter before the planning call.",
  "Draft reply: thanks for the detailed notes. We'll test the new flow on our side this week and follow up with feedback by Monday.",
  "Add milk, coffee beans and a new notebook to the shopping list.",
  "The build passed on the second try. The flaky test is the one that waits for the network mock, so let's give it a longer timeout.",
  "Summary of today: finished the layout pass, fixed two spacing bugs in the sidebar, and started on the settings dialog.",
  "Remind me to call the dentist tomorrow morning to reschedule the appointment.",
];

const day = 24 * 60 * 60;

const makeHistory = (): HistoryEntry[] => {
  const now = Math.floor(Date.now() / 1000);
  const offsets = [
    600,
    2 * 3600,
    day + 3600,
    day + 5 * 3600,
    day + 7 * 3600,
    2 * day + 3600,
    3 * day,
    5 * day,
  ];
  return offsets.map((offset, i) => ({
    id: 100 - i,
    file_name: `sample-${i}.wav`,
    timestamp: now - offset,
    saved: i === 1,
    title: `Recording ${i + 1}`,
    transcription_text: SAMPLE_TEXTS[i % SAMPLE_TEXTS.length],
    post_processed_text: null,
    post_process_prompt: null,
    post_process_requested: false,
  }));
};

const MODEL: ModelInfo = {
  id: "parakeet-tdt-0.6b-v3",
  name: "Parakeet V3",
  description: "Fast and accurate English model",
  filename: "parakeet-tdt-0.6b-v3",
  source: { Url: { url: "" } } as unknown as ModelInfo["source"],
  size_mb: 478,
  is_downloaded: true,
  is_downloading: false,
  partial_size: 0,
  accuracy_score: 0.85,
  speed_score: 0.9,
  supports_translation: false,
  is_recommended: true,
  supported_languages: ["en"],
  supports_language_selection: false,
  is_custom: false,
  supports_streaming: false,
  supports_language_detection: false,
};

const MIC: AudioDevice = {
  index: "0",
  name: "MacBook Pro Microphone",
  is_default: true,
};
const SPEAKERS: AudioDevice = {
  index: "0",
  name: "MacBook Pro Speakers",
  is_default: true,
};

const makeSettings = (theme: "dark" | "light"): AppSettings => ({
  bindings: {
    transcribe: {
      id: "transcribe",
      name: "Transcribe",
      description: "Converts your speech into text.",
      default_binding: "fn",
      current_binding: "fn",
    },
    transcribe_handsfree: {
      id: "transcribe_handsfree",
      name: "Hands-free",
      description: "Starts dictation that keeps going until you stop it.",
      default_binding: "fn+space",
      current_binding: "fn+space",
    },
    cancel: {
      id: "cancel",
      name: "Cancel",
      description: "Cancels the current recording.",
      default_binding: "escape",
      current_binding: "escape",
    },
  },
  shortcut_activation: "toggle",
  audio_feedback: true,
  audio_feedback_volume: 1,
  start_hidden: false,
  autostart_enabled: true,
  update_checks_enabled: true,
  show_whats_new_on_update: false,
  whats_new_last_seen_version: "0.1.1",
  selected_model: MODEL.id,
  onboarding_completed: true,
  always_on_microphone: false,
  selected_microphone: null,
  selected_output_device: null,
  translate_to_english: false,
  selected_language: "auto",
  debug_mode: false,
  custom_words: [APP_NAME, "Ollama", "Tauri"],
  history_limit: 100,
  post_process_enabled: false,
  post_process_prompts: [],
  mute_while_recording: true,
  append_trailing_space: true,
  app_language: "en",
  theme,
  experimental_enabled: false,
  show_tray_icon: true,
  overlay_style: "minimal",
});

const defaults = (theme: "dark" | "light"): Record<string, MockHandler> => {
  const settings = makeSettings(theme);
  const history = makeHistory();
  return {
    get_app_settings: () => settings,
    get_default_settings: () => settings,
    get_history_entries: () => ({ entries: history, has_more: false }),
    get_available_models: () => [MODEL],
    get_model_info: () => MODEL,
    get_current_model: () => MODEL.id,
    get_retired_models: () => [],
    get_transcription_model_status: () => MODEL.id,
    get_model_load_status: () => ({ is_loaded: true, current_model: MODEL.id }),
    is_model_loading: () => false,
    get_available_microphones: () => [MIC],
    get_selected_microphone: () => MIC.name,
    get_available_output_devices: () => [SPEAKERS],
    get_selected_output_device: () => SPEAKERS.name,
    get_clamshell_microphone: () => null,
    get_microphone_channels: () => 1,
    get_microphone_mode: () => false,
    is_laptop: () => true,
    is_recording: () => false,
    is_portable: () => false,
    is_update_checks_locked: () => false,
    check_custom_sounds: () => ({ start: false, stop: false }),
    // ?secureInput=<process> shows the paused-shortcut notice.
    get_secure_input_status: () => {
      const culprit = new URLSearchParams(window.location.search).get(
        "secureInput",
      );
      return {
        enabled: culprit !== null,
        sustained: culprit !== null,
        culprit_pid: culprit ? 398 : null,
        culprit_name: culprit || null,
        fallback_active: culprit !== null,
        covered_bindings: [],
        degraded_bindings: [],
        uncovered_bindings: culprit ? ["transcribe_handsfree"] : [],
        recorder_blocked: false,
      };
    },
    get_keyboard_implementation: () => "tauri",
    get_available_typing_tools: () => [],
    get_available_accelerators: () => ({ transcribe: [], gpu_devices: [] }),
    get_windows_microphone_permission_status: () => ({
      supported: false,
      overall_access: "allowed",
    }),
    get_app_dir_path: () =>
      `/Users/you/Library/Application Support/${APP_NAME}`,
    get_log_dir_path: () => `/Users/you/Library/Logs/${APP_NAME}`,
    check_apple_intelligence_available: () => false,
    get_audio_file_path: () => "",
    "plugin:app|version": () => "0.1.1",
    "plugin:app|name": () => APP_NAME,
    "plugin:event|listen": () => Math.floor(Math.random() * 1e6),
    "plugin:event|unlisten": () => null,
    "plugin:event|emit": () => null,
    "plugin:macos-permissions|check_accessibility_permission": () => true,
    "plugin:macos-permissions|check_microphone_permission": () => true,
    "plugin:autostart|is_enabled": () => true,
    "plugin:store|load": () => 1,
    "plugin:store|get": () => [null, false],
  };
};

const extraHandlers = (): Record<string, MockHandler> => {
  const modules = import.meta.glob<{ handlers?: Record<string, MockHandler> }>(
    "./mocks/*.ts",
    { eager: true },
  );
  return Object.assign(
    {},
    ...Object.values(modules).map((m) => m.handlers ?? {}),
  );
};

export const installMockTauri = ({ theme }: { theme: "dark" | "light" }) => {
  const handlers = { ...defaults(theme), ...extraHandlers() };
  const w = window as unknown as Record<string, unknown>;
  let nextCallback = 1;

  w.__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
      const handler = handlers[cmd];
      if (handler) return handler(args);
      if (!cmd.startsWith("change_") && !cmd.startsWith("set_")) {
        console.warn(`[flowPreview] unmocked command: ${cmd}`, args);
      }
      return null;
    },
    transformCallback: (callback: (v: unknown) => void) => {
      const id = nextCallback++;
      w[`_${id}`] = callback;
      return id;
    },
    unregisterCallback: (id: number) => {
      delete w[`_${id}`];
    },
    convertFileSrc: (path: string) => path,
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { windowLabel: "main", label: "main" },
    },
    plugins: {},
  };
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  w.__TAURI_OS_PLUGIN_INTERNALS__ = {
    platform: "macos",
    os_type: "macos",
    family: "unix",
    arch: "aarch64",
    version: "26.0",
    eol: "\n",
    exe_extension: "",
  };
};
