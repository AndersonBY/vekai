import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AppBootstrap,
  AppConfig,
  AppSnapshot,
  JsonValue,
  ServiceConfig,
  ServiceSnapshot,
  ServiceStatus,
} from "./types";

type BusyAction =
  | "bootstrap"
  | "save"
  | "start"
  | "stop"
  | "restart"
  | "test-rerank"
  | "test-embedding"
  | "browse-rerank"
  | "browse-embedding"
  | "open-logs";

type ServiceId = "rerank" | "embedding";
type Language = "zh" | "en";

const EMPTY_CONFIG: AppConfig = {
  host: "127.0.0.1",
  rerank: { enabled: true, port: 8012, model_path: "", protocol: "llama_rerank" },
  embedding: { enabled: true, port: 8013, model_path: "", protocol: "openai_embeddings" },
  auto_start_server: true,
  restart_on_crash: true,
  gpu_layers: "auto",
  threads: 0,
  parallel_slots: 0,
  http_threads: 0,
  log_retention_days: 7,
};

const GPU_LAYER_PRESETS = ["auto", "all", "0"] as const;
const SERVICE_PROTOCOL_OPTIONS: Record<ServiceId, string[]> = {
  rerank: [
    "llama_rerank",
    "siliconflow",
    "cohere_rerank_v2",
    "jina_rerank_v1",
    "voyage_rerank_v1",
    "custom_json_http",
  ],
  embedding: [
    "openai_embeddings",
    "siliconflow",
    "cohere_embed_v2",
    "voyage_embeddings_v1",
    "custom_json_http",
  ],
};
const SERVICE_IMPACTING_CONFIG_FIELDS: Array<keyof AppConfig> = [
  "host",
  "rerank",
  "embedding",
  "gpu_layers",
  "threads",
  "parallel_slots",
  "http_threads",
];

const TEXT = {
  zh: {
    appTitle: "Vekai",
    subtitle: "llama.cpp 嵌入与重排序服务",
    language: "界面语言",
    chinese: "中文",
    english: "English",
    rerank: "重排序",
    embedding: "嵌入",
    logs: "日志",
    unavailable: "不可用",
    closeTitle: "关闭设置窗口",
    closeDescription: "要把设置窗口最小化到系统托盘，还是退出程序并停止本地服务？",
    minimizeToTray: "最小化到托盘",
    quitApp: "退出程序",
    sharedSettings: "通用设置",
    host: "监听地址",
    gpuLayers: "GPU 层数",
    custom: "自定义",
    threads: "线程数",
    parallelSlots: "并发槽位",
    httpThreads: "HTTP 线程数",
    logRetentionDays: "日志保留天数",
    autoStart: "启动后自动开启已启用服务",
    restartOnCrash: "崩溃后自动重启",
    startServices: "启动服务",
    stopServices: "停止服务",
    restartServices: "重启服务",
    browseModel: "选择文件",
    save: "保存",
    saveRestart: "保存并重启",
    openLogs: "打开日志目录",
    enabled: "启用",
    port: "端口",
    protocol: "接口协议",
    model: "模型",
    modelPath: "模型文件路径",
    noModelSelected: "未选择模型",
    test: "测试",
    recentLogs: "最近日志",
    lines: "行",
    noLogs: "暂无日志。",
    json: "JSON",
    statuses: {
      Stopped: "已停止",
      Starting: "启动中",
      Running: "运行中",
      Stopping: "停止中",
      Failed: "失败",
    },
    busy: {
      bootstrap: "正在加载...",
      save: "正在保存...",
      saveRestart: "正在保存并重启...",
      start: "正在启动...",
      stop: "正在停止...",
      restart: "正在重启...",
      test: "正在测试",
      openLogs: "正在打开...",
      browseModel: "正在选择...",
    },
    messages: {
      loaded: "设置已加载。",
      saved: "配置已保存。",
      savedRestarted: "配置已保存，服务已重启。",
      savedRestartFailed: "配置已保存，但重启失败。",
      servicesStarted: "服务已启动。",
      servicesStopped: "服务已停止。",
      servicesRestarted: "服务已重启。",
      testCompleted: (title: string) => `${title} 测试完成。`,
      logsOpened: "已在资源管理器中打开日志目录。",
      bootstrapFailed: (message: string) => `加载设置失败：${message}`,
      noModel: (title: string) => `${title} 服务未选择模型。`,
      bindAll: "当前监听所有网卡，只建议在可信网络中使用。",
      duplicatePorts: "重排序和嵌入服务不能使用同一个端口。",
      rerankPlaceholder: "重排序服务运行后可执行测试。",
      embeddingPlaceholder: "嵌入服务运行后可执行测试。",
      modelSelected: (title: string) => `已选择 ${title} 模型文件。`,
    },
    protocolLabels: {
      llama_rerank: "llama.cpp 原生",
      openai_embeddings: "OpenAI",
      siliconflow: "SiliconFlow",
      cohere_embed_v2: "Cohere v2",
      cohere_rerank_v2: "Cohere v2",
      jina_rerank_v1: "Jina v1",
      voyage_embeddings_v1: "Voyage v1",
      voyage_rerank_v1: "Voyage v1",
      custom_json_http: "自定义 JSON HTTP",
    },
  },
  en: {
    appTitle: "Vekai",
    subtitle: "llama.cpp embedding and rerank services",
    language: "Language",
    chinese: "中文",
    english: "English",
    rerank: "Rerank",
    embedding: "Embedding",
    logs: "Logs",
    unavailable: "Unavailable",
    closeTitle: "Close Settings",
    closeDescription: "Minimize the settings window to the system tray, or quit the app and stop local services?",
    minimizeToTray: "Minimize to Tray",
    quitApp: "Quit App",
    sharedSettings: "Shared Settings",
    host: "Host",
    gpuLayers: "GPU layers",
    custom: "Custom",
    threads: "Threads",
    parallelSlots: "Parallel slots",
    httpThreads: "HTTP threads",
    logRetentionDays: "Log retention days",
    autoStart: "Auto-start enabled services",
    restartOnCrash: "Restart on crash",
    startServices: "Start Services",
    stopServices: "Stop Services",
    restartServices: "Restart Services",
    browseModel: "Browse",
    save: "Save",
    saveRestart: "Save & Restart",
    openLogs: "Open Logs Folder",
    enabled: "Enabled",
    port: "Port",
    protocol: "Protocol",
    model: "Model",
    modelPath: "Model file path",
    noModelSelected: "No model selected",
    test: "Test",
    recentLogs: "Recent Logs",
    lines: "line(s)",
    noLogs: "No logs yet.",
    json: "JSON",
    statuses: {
      Stopped: "Stopped",
      Starting: "Starting",
      Running: "Running",
      Stopping: "Stopping",
      Failed: "Failed",
    },
    busy: {
      bootstrap: "Loading...",
      save: "Saving...",
      saveRestart: "Saving & Restarting...",
      start: "Starting...",
      stop: "Stopping...",
      restart: "Restarting...",
      test: "Testing",
      openLogs: "Opening...",
      browseModel: "Browsing...",
    },
    messages: {
      loaded: "Settings loaded.",
      saved: "Configuration saved.",
      savedRestarted: "Configuration saved and services restarted.",
      savedRestartFailed: "Configuration saved, but restart failed.",
      servicesStarted: "Services started.",
      servicesStopped: "Services stopped.",
      servicesRestarted: "Services restarted.",
      testCompleted: (title: string) => `${title} test completed.`,
      logsOpened: "Opened logs folder in Explorer.",
      bootstrapFailed: (message: string) => `Failed to load settings: ${message}`,
      noModel: (title: string) => `${title} service has no model selected.`,
      bindAll: "Host binds on all interfaces. Use only on a trusted network.",
      duplicatePorts: "Rerank and embedding services cannot use the same port.",
      rerankPlaceholder: "Run Test Rerank while the rerank service is running.",
      embeddingPlaceholder: "Run Test Embedding while the embedding service is running.",
      modelSelected: (title: string) => `Selected ${title} model file.`,
    },
    protocolLabels: {
      llama_rerank: "llama.cpp native",
      openai_embeddings: "OpenAI",
      siliconflow: "SiliconFlow",
      cohere_embed_v2: "Cohere v2",
      cohere_rerank_v2: "Cohere v2",
      jina_rerank_v1: "Jina v1",
      voyage_embeddings_v1: "Voyage v1",
      voyage_rerank_v1: "Voyage v1",
      custom_json_http: "Custom JSON HTTP",
    },
  },
} as const;

const SERVICE_META: Record<
  ServiceId,
  {
    labelKey: "rerank" | "embedding";
    testAction: Extract<BusyAction, "test-rerank" | "test-embedding">;
    testCommand: "test_rerank" | "test_embedding";
    browseAction: Extract<BusyAction, "browse-rerank" | "browse-embedding">;
  }
> = {
  rerank: {
    labelKey: "rerank",
    testAction: "test-rerank",
    testCommand: "test_rerank",
    browseAction: "browse-rerank",
  },
  embedding: {
    labelKey: "embedding",
    testAction: "test-embedding",
    testCommand: "test_embedding",
    browseAction: "browse-embedding",
  },
};

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function formatJson(value: JsonValue | null): string {
  return value === null ? "" : JSON.stringify(value, null, 2);
}

function isServerTransitioning(status: ServiceStatus): boolean {
  return status === "Starting" || status === "Stopping";
}

function isBindAllHost(host: string): boolean {
  const normalized = host.trim();
  return normalized === "0.0.0.0" || normalized === "::";
}

function isGpuLayerPreset(value: string): boolean {
  return GPU_LAYER_PRESETS.some((preset) => preset === value);
}

function hasServiceImpactingConfigChange(previous: AppConfig, next: AppConfig): boolean {
  return SERVICE_IMPACTING_CONFIG_FIELDS.some(
    (field) => JSON.stringify(previous[field]) !== JSON.stringify(next[field]),
  );
}

function statusTone(status: ServiceStatus): string {
  switch (status) {
    case "Running":
      return "good";
    case "Starting":
    case "Stopping":
      return "pending";
    case "Failed":
      return "bad";
    case "Stopped":
    default:
      return "neutral";
  }
}

function anyServiceTransitioning(snapshot: AppSnapshot | null): boolean {
  return (
    isServerTransitioning(snapshot?.rerank.status ?? "Stopped") ||
    isServerTransitioning(snapshot?.embedding.status ?? "Stopped")
  );
}

function anyServiceRunning(snapshot: AppSnapshot | null): boolean {
  return snapshot?.rerank.status === "Running" || snapshot?.embedding.status === "Running";
}

function allEnabledStopped(snapshot: AppSnapshot | null, config: AppConfig): boolean {
  if (snapshot === null) return true;
  return (
    (!config.rerank.enabled || snapshot.rerank.status === "Stopped") &&
    (!config.embedding.enabled || snapshot.embedding.status === "Stopped")
  );
}

function endpoint(config: AppConfig, service: ServiceId): string {
  return `${config.host || "127.0.0.1"}:${Number(config[service].port) || 0}`;
}

function defaultProtocol(service: ServiceId): string {
  return service === "rerank" ? "llama_rerank" : "openai_embeddings";
}

function ServicePanel(props: {
  service: ServiceId;
  snapshot: ServiceSnapshot;
  config: AppConfig;
  busyAction: BusyAction | null;
  disabled: boolean;
  result: JsonValue | null;
  language: Language;
  onServiceConfigChange: (service: ServiceId, value: ServiceConfig) => void;
  onTest: (service: ServiceId) => void;
  onBrowseModel: (service: ServiceId) => void;
}) {
  const { service, snapshot, config, busyAction, disabled, language } = props;
  const t = TEXT[language];
  const meta = SERVICE_META[service];
  const title = t[meta.labelKey];
  const serviceConfig = config[service];
  const resultText =
    formatJson(props.result) ||
    (service === "rerank" ? t.messages.rerankPlaceholder : t.messages.embeddingPlaceholder);
  const canTest = !disabled && serviceConfig.enabled && snapshot.status === "Running" && busyAction === null;
  const canBrowse = !disabled && busyAction === null;

  return (
    <section className="panel service-panel">
      <div className="panel-header">
        <div>
          <h2>{title}</h2>
          <p className="muted">{endpoint(config, service)}</p>
        </div>
        <div className={`status-pill ${statusTone(snapshot.status)}`}>
          <span className="status-dot" aria-hidden="true" />
          {t.statuses[snapshot.status]}
        </div>
      </div>

      <div className="service-grid">
        <label className="toggle-row inline-toggle">
          <input
            type="checkbox"
            checked={serviceConfig.enabled}
            disabled={disabled}
            onChange={(event) =>
              props.onServiceConfigChange(service, {
                ...serviceConfig,
                enabled: event.target.checked,
              })
            }
          />
          {t.enabled}
        </label>
        <label>
          {t.port}
          <input
            type="number"
            min={1}
            max={65535}
            value={Number(serviceConfig.port)}
            disabled={disabled}
            onChange={(event) =>
              props.onServiceConfigChange(service, {
                ...serviceConfig,
                port: Number(event.target.value),
              })
            }
          />
        </label>
        <label>
          {t.protocol}
          <select
            value={serviceConfig.protocol || defaultProtocol(service)}
            disabled={disabled}
            onChange={(event) =>
              props.onServiceConfigChange(service, {
                ...serviceConfig,
                protocol: event.target.value,
              })
            }
          >
            {SERVICE_PROTOCOL_OPTIONS[service].map((protocol) => (
              <option key={protocol} value={protocol}>
                {t.protocolLabels[protocol as keyof typeof t.protocolLabels]}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className="model-path-row">
        <label className="service-model">
          {t.modelPath}
          <input
            value={serviceConfig.model_path}
            placeholder={t.noModelSelected}
            spellCheck={false}
            disabled={disabled}
            onChange={(event) =>
              props.onServiceConfigChange(service, {
                ...serviceConfig,
                model_path: event.target.value,
              })
            }
          />
        </label>
        <button type="button" onClick={() => props.onBrowseModel(service)} disabled={!canBrowse}>
          {busyAction === meta.browseAction ? t.busy.browseModel : t.browseModel}
        </button>
      </div>

      {snapshot.error ? <div className="notice error">{snapshot.error}</div> : null}

      <div className="service-actions">
        <button type="button" onClick={() => props.onTest(service)} disabled={!canTest}>
          {busyAction === meta.testAction ? `${t.busy.test} ${title}...` : `${t.test} ${title}`}
        </button>
      </div>

      <pre className="result-view service-result">{resultText}</pre>
    </section>
  );
}

export default function App() {
  const [language, setLanguage] = useState<Language>("zh");
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [config, setConfig] = useState<AppConfig>(EMPTY_CONFIG);
  const [logsDir, setLogsDir] = useState("");
  const [bootstrapWarning, setBootstrapWarning] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [rerankResult, setRerankResult] = useState<JsonValue | null>(null);
  const [embeddingResult, setEmbeddingResult] = useState<JsonValue | null>(null);
  const [closePromptOpen, setClosePromptOpen] = useState(false);
  const [busyAction, setBusyAction] = useState<BusyAction | null>("bootstrap");
  const busyActionRef = useRef<BusyAction | null>("bootstrap");
  const t = TEXT[language];

  const fallbackSnapshot: AppSnapshot = {
    config,
    rerank: { status: "Stopped", config: config.rerank, recent_logs: [], error: null },
    embedding: { status: "Stopped", config: config.embedding, recent_logs: [], error: null },
    recent_logs: [],
    error: null,
  };
  const currentSnapshot = snapshot ?? fallbackSnapshot;
  const recentLogs = [
    ...currentSnapshot.rerank.recent_logs.map((line) => `[${t.rerank}] ${line}`),
    ...currentSnapshot.embedding.recent_logs.map((line) => `[${t.embedding}] ${line}`),
  ];
  const serverBusy = busyAction !== null || anyServiceTransitioning(snapshot);
  const saveRestartsServer =
    anyServiceRunning(snapshot) &&
    snapshot !== null &&
    hasServiceImpactingConfigChange(snapshot.config, config);
  const canStart =
    !serverBusy &&
    (config.rerank.enabled || config.embedding.enabled) &&
    allEnabledStopped(snapshot, config);
  const canStop = !serverBusy && anyServiceRunning(snapshot);
  const canRestart = !serverBusy && anyServiceRunning(snapshot);
  const hasLogsDir = logsDir.trim().length > 0;
  const gpuLayerMode = isGpuLayerPreset(config.gpu_layers) ? config.gpu_layers : "custom";
  const saveButtonLabel =
    busyAction === "save"
      ? saveRestartsServer
        ? t.busy.saveRestart
        : t.busy.save
      : saveRestartsServer
        ? t.saveRestart
        : t.save;
  const recentLogText =
    recentLogs.length > 0 ? recentLogs.slice(-100).join("\n") : t.noLogs;
  const hostWarning = isBindAllHost(config.host) ? t.messages.bindAll : null;
  const portWarning =
    config.rerank.enabled &&
    config.embedding.enabled &&
    Number(config.rerank.port) === Number(config.embedding.port)
      ? t.messages.duplicatePorts
      : null;
  const modelWarnings = (["rerank", "embedding"] as ServiceId[])
    .map((service) => {
      const title = t[SERVICE_META[service].labelKey];
      const serviceConfig = config[service];
      if (!serviceConfig.enabled) return null;
      if (!serviceConfig.model_path.trim()) return t.messages.noModel(title);
      return null;
    })
    .filter(Boolean);
  const bootstrapNotice =
    bootstrapWarning && bootstrapWarning !== currentSnapshot.error ? bootstrapWarning : null;

  useEffect(() => {
    let cancelled = false;
    async function loadBootstrap() {
      setCurrentBusyAction("bootstrap");
      setError(null);
      try {
        const bootstrap = await invoke<AppBootstrap>("get_bootstrap");
        if (cancelled) return;
        setSnapshot(bootstrap.snapshot);
        setConfig(bootstrap.snapshot.config);
        setLogsDir(bootstrap.logs_dir);
        setBootstrapWarning(bootstrap.warning);
        setMessage(TEXT.zh.messages.loaded);
      } catch (caught) {
        if (!cancelled) setError(TEXT.zh.messages.bootstrapFailed(getErrorMessage(caught)));
      } finally {
        if (!cancelled) setCurrentBusyAction(null);
      }
    }
    void loadBootstrap();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    function handleCloseRequested() {
      setClosePromptOpen(true);
    }

    window.addEventListener("vekai-close-requested", handleCloseRequested);
    return () => {
      window.removeEventListener("vekai-close-requested", handleCloseRequested);
    };
  }, []);

  function setCurrentBusyAction(action: BusyAction | null) {
    busyActionRef.current = action;
    setBusyAction(action);
  }

  function beginBusyAction(action: BusyAction): boolean {
    if (busyActionRef.current !== null) return false;
    setCurrentBusyAction(action);
    return true;
  }

  function updateConfig<Key extends keyof AppConfig>(key: Key, value: AppConfig[Key]) {
    setConfig((current) => ({ ...current, [key]: value }));
  }

  function updateServiceConfig(service: ServiceId, value: ServiceConfig) {
    setConfig((current) => ({ ...current, [service]: value }));
  }

  function applySnapshot(nextSnapshot: AppSnapshot, nextMessage: string) {
    setSnapshot(nextSnapshot);
    setConfig(nextSnapshot.config);
    setMessage(nextMessage);
    setError(null);
  }

  async function runSnapshotCommand(
    action: BusyAction,
    command: "start_servers" | "stop_servers" | "restart_servers",
    successMessage: string,
  ) {
    if (!beginBusyAction(action)) return;
    setError(null);
    setMessage(null);
    try {
      applySnapshot(await invoke<AppSnapshot>(command), successMessage);
    } catch (caught) {
      setError(getErrorMessage(caught));
    } finally {
      setCurrentBusyAction(null);
    }
  }

  async function browseModel(service: ServiceId) {
    const meta = SERVICE_META[service];
    if (!beginBusyAction(meta.browseAction)) return;
    setError(null);
    setMessage(null);
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "GGUF", extensions: ["gguf"] }],
      });
      if (typeof selected === "string") {
        updateServiceConfig(service, {
          ...config[service],
          model_path: selected,
        });
        setMessage(t.messages.modelSelected(t[meta.labelKey]));
      }
    } catch (caught) {
      setError(getErrorMessage(caught));
    } finally {
      setCurrentBusyAction(null);
    }
  }

  async function saveConfig() {
    if (!beginBusyAction("save")) return;
    const configToSave = config;
    const shouldRestartAfterSave =
      anyServiceRunning(snapshot) &&
      snapshot !== null &&
      hasServiceImpactingConfigChange(snapshot.config, configToSave);
    let savedSnapshot: AppSnapshot | null = null;
    setError(null);
    setMessage(null);
    try {
      savedSnapshot = await invoke<AppSnapshot>("save_config", { config: configToSave });
      if (shouldRestartAfterSave) {
        applySnapshot(await invoke<AppSnapshot>("restart_servers"), t.messages.savedRestarted);
        return;
      }
      applySnapshot(savedSnapshot, t.messages.saved);
    } catch (caught) {
      if (savedSnapshot !== null) {
        try {
          const bootstrap = await invoke<AppBootstrap>("get_bootstrap");
          setSnapshot(bootstrap.snapshot);
          setConfig(bootstrap.snapshot.config);
          setLogsDir(bootstrap.logs_dir);
          setBootstrapWarning(bootstrap.warning);
        } catch {
          setSnapshot(savedSnapshot);
          setConfig(savedSnapshot.config);
        }
        setMessage(t.messages.savedRestartFailed);
      }
      setError(getErrorMessage(caught));
    } finally {
      setCurrentBusyAction(null);
    }
  }

  async function testService(service: ServiceId) {
    const meta = SERVICE_META[service];
    if (!beginBusyAction(meta.testAction)) return;
    setError(null);
    setMessage(null);
    service === "rerank" ? setRerankResult(null) : setEmbeddingResult(null);
    try {
      const result = await invoke<JsonValue>(meta.testCommand);
      service === "rerank" ? setRerankResult(result) : setEmbeddingResult(result);
      setMessage(t.messages.testCompleted(t[meta.labelKey]));
    } catch (caught) {
      setError(getErrorMessage(caught));
    } finally {
      setCurrentBusyAction(null);
    }
  }

  async function openLogsFolder() {
    const targetPath = logsDir.trim();
    if (!targetPath || !beginBusyAction("open-logs")) return;
    setError(null);
    setMessage(null);
    try {
      await invoke("plugin:opener|reveal_item_in_dir", { paths: [targetPath] });
      setMessage(t.messages.logsOpened);
    } catch (caught) {
      setError(getErrorMessage(caught));
    } finally {
      setCurrentBusyAction(null);
    }
  }

  async function minimizeSettingsToTray() {
    setClosePromptOpen(false);
    try {
      await invoke("hide_settings_window");
    } catch (caught) {
      setError(getErrorMessage(caught));
    }
  }

  async function quitApplication() {
    setClosePromptOpen(false);
    try {
      await invoke("quit_app");
    } catch (caught) {
      setError(getErrorMessage(caught));
    }
  }

  return (
    <main className="app-shell">
      <header className="top-bar">
        <div>
          <h1>{t.appTitle}</h1>
          <p className="subtitle">{t.subtitle}</p>
        </div>
        <div className="top-controls">
          <label className="language-select">
            {t.language}
            <select value={language} onChange={(event) => setLanguage(event.target.value as Language)}>
              <option value="zh">{t.chinese}</option>
              <option value="en">{t.english}</option>
            </select>
          </label>
          <div className="top-status">
            <div className={`status-pill ${statusTone(currentSnapshot.rerank.status)}`}>
              <span className="status-dot" aria-hidden="true" />
              {t.rerank} {t.statuses[currentSnapshot.rerank.status]}
            </div>
            <div className={`status-pill ${statusTone(currentSnapshot.embedding.status)}`}>
              <span className="status-dot" aria-hidden="true" />
              {t.embedding} {t.statuses[currentSnapshot.embedding.status]}
            </div>
          </div>
        </div>
      </header>

      <section className="summary-grid" aria-label={t.sharedSettings}>
        <div className="summary-item">
          <span>{t.rerank}</span>
          <strong>{endpoint(config, "rerank")}</strong>
        </div>
        <div className="summary-item">
          <span>{t.embedding}</span>
          <strong>{endpoint(config, "embedding")}</strong>
        </div>
        <div className="summary-item wide">
          <span>{t.logs}</span>
          <strong title={logsDir}>{logsDir || t.unavailable}</strong>
        </div>
      </section>

      <div className="message-stack" aria-live="polite">
        {message ? <div className="notice success">{message}</div> : null}
        {bootstrapNotice ? <div className="notice warning">{bootstrapNotice}</div> : null}
        {hostWarning ? <div className="notice warning">{hostWarning}</div> : null}
        {portWarning ? <div className="notice warning">{portWarning}</div> : null}
        {modelWarnings.map((warning) => (
          <div key={warning} className="notice warning">
            {warning}
          </div>
        ))}
        {currentSnapshot.error ? <div className="notice error">{currentSnapshot.error}</div> : null}
        {error ? <div className="notice error">{error}</div> : null}
      </div>

      <section className="panel">
        <div className="panel-header">
          <h2>{t.sharedSettings}</h2>
          <div className="button-row">
            <button type="button" className="primary" onClick={() => void runSnapshotCommand("start", "start_servers", t.messages.servicesStarted)} disabled={!canStart}>
              {busyAction === "start" ? t.busy.start : t.startServices}
            </button>
            <button type="button" onClick={() => void runSnapshotCommand("stop", "stop_servers", t.messages.servicesStopped)} disabled={!canStop}>
              {busyAction === "stop" ? t.busy.stop : t.stopServices}
            </button>
            <button type="button" onClick={() => void runSnapshotCommand("restart", "restart_servers", t.messages.servicesRestarted)} disabled={!canRestart}>
              {busyAction === "restart" ? t.busy.restart : t.restartServices}
            </button>
          </div>
        </div>

        <div className="form-grid shared-grid">
          <label>
            {t.host}
            <input value={config.host} onChange={(event) => updateConfig("host", event.target.value)} spellCheck={false} />
          </label>
          <label>
            {t.gpuLayers}
            <div className="combo-field">
              <select
                aria-label={t.gpuLayers}
                value={gpuLayerMode}
                onChange={(event) => {
                  const nextValue = event.target.value;
                  updateConfig("gpu_layers", nextValue === "custom" ? "" : nextValue);
                }}
              >
                {GPU_LAYER_PRESETS.map((preset) => (
                  <option key={preset} value={preset}>
                    {preset}
                  </option>
                ))}
                <option value="custom">{t.custom}</option>
              </select>
              <input
                aria-label={t.custom}
                type="number"
                min={0}
                step={1}
                value={gpuLayerMode === "custom" ? config.gpu_layers : ""}
                onChange={(event) => updateConfig("gpu_layers", event.target.value.trim())}
                placeholder={t.custom}
              />
            </div>
          </label>
          <label>
            {t.threads}
            <input type="number" min={0} max={65535} value={config.threads} onChange={(event) => updateConfig("threads", Number(event.target.value))} />
          </label>
          <label>
            {t.parallelSlots}
            <input type="number" min={0} max={65535} value={config.parallel_slots} onChange={(event) => updateConfig("parallel_slots", Number(event.target.value))} />
          </label>
          <label>
            {t.httpThreads}
            <input type="number" min={0} max={65535} value={config.http_threads} onChange={(event) => updateConfig("http_threads", Number(event.target.value))} />
          </label>
          <label>
            {t.logRetentionDays}
            <input type="number" min={1} max={365} value={config.log_retention_days} onChange={(event) => updateConfig("log_retention_days", Number(event.target.value))} />
          </label>
        </div>

        <div className="toggle-grid">
          <label className="toggle-row">
            <input type="checkbox" checked={config.auto_start_server} onChange={(event) => updateConfig("auto_start_server", event.target.checked)} />
            {t.autoStart}
          </label>
          <label className="toggle-row">
            <input type="checkbox" checked={config.restart_on_crash} onChange={(event) => updateConfig("restart_on_crash", event.target.checked)} />
            {t.restartOnCrash}
          </label>
        </div>

        <div className="footer-actions">
          <button type="button" className="primary" onClick={() => void saveConfig()} disabled={serverBusy}>
            {saveButtonLabel}
          </button>
          {hasLogsDir ? (
            <button type="button" onClick={() => void openLogsFolder()} disabled={serverBusy}>
              {busyAction === "open-logs" ? t.busy.openLogs : t.openLogs}
            </button>
          ) : null}
        </div>
      </section>

      <section className="service-columns">
        <ServicePanel
          service="rerank"
          snapshot={currentSnapshot.rerank}
          config={config}
          busyAction={busyAction}
          disabled={serverBusy}
          result={rerankResult}
          language={language}
          onServiceConfigChange={updateServiceConfig}
          onTest={(service) => void testService(service)}
          onBrowseModel={(service) => void browseModel(service)}
        />
        <ServicePanel
          service="embedding"
          snapshot={currentSnapshot.embedding}
          config={config}
          busyAction={busyAction}
          disabled={serverBusy}
          result={embeddingResult}
          language={language}
          onServiceConfigChange={updateServiceConfig}
          onTest={(service) => void testService(service)}
          onBrowseModel={(service) => void browseModel(service)}
        />
      </section>

      <section className="panel">
        <div className="panel-header">
          <h2>{t.recentLogs}</h2>
          <span className="muted">{recentLogs.length} {t.lines}</span>
        </div>
        <pre className="log-view">{recentLogText}</pre>
      </section>

      {closePromptOpen ? (
        <div className="modal-backdrop" role="presentation">
          <section className="modal" role="dialog" aria-modal="true" aria-labelledby="close-dialog-title">
            <h2 id="close-dialog-title">{t.closeTitle}</h2>
            <p>{t.closeDescription}</p>
            <div className="modal-actions">
              <button type="button" className="primary" onClick={() => void minimizeSettingsToTray()}>
                {t.minimizeToTray}
              </button>
              <button type="button" onClick={() => void quitApplication()}>
                {t.quitApp}
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </main>
  );
}
