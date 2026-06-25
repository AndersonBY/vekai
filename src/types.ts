export type ServiceStatus =
  | "Stopped"
  | "Starting"
  | "Running"
  | "Stopping"
  | "Failed";

export interface AppConfig {
  host: string;
  rerank: ServiceConfig;
  embedding: ServiceConfig;
  auto_start_server: boolean;
  restart_on_crash: boolean;
  gpu_layers: string;
  threads: number;
  log_retention_days: number;
}

export interface ServiceConfig {
  enabled: boolean;
  port: number;
  model_path: string;
  protocol: string;
}

export interface ModelInfo {
  name: string;
  path: string;
  size_bytes: number;
  selected: boolean;
}

export interface ServiceSnapshot {
  status: ServiceStatus;
  config: ServiceConfig;
  recent_logs: string[];
  error: string | null;
}

export interface AppSnapshot {
  config: AppConfig;
  rerank: ServiceSnapshot;
  embedding: ServiceSnapshot;
  recent_logs: string[];
  error: string | null;
}

export interface ModelCatalog {
  rerank: ModelInfo[];
  embedding: ModelInfo[];
}

export interface AppBootstrap {
  snapshot: AppSnapshot;
  models: ModelCatalog;
  logs_dir: string;
  warning: string | null;
}

export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };
