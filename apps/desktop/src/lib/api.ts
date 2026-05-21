import { invoke } from "@tauri-apps/api/core";

export interface ProtectionStatus {
  enabled: boolean;
  dns_provider: string;
  uptime_seconds: number;
  total_rules: number;
}

export interface StatsResponse {
  total_queries: number;
  blocked_queries: number;
  cache_hits: number;
  block_percentage: number;
  uptime_seconds: number;
}

export interface AppConfig {
  dns_provider_id: string;
  auto_start: boolean;
  dark_mode: boolean;
  cache_size: number;
  listen_address: string;
  port: number;
}

export interface DnsProvider {
  id: string;
  name: string;
  primary: string;
  secondary: string;
  description: string;
}

export interface QueryEvent {
  timestamp: number;
  domain: string;
  query_type: string;
  status: "allowed" | "blocked" | "cached" | "error";
  response_time_ms: number;
  upstream: string;
  rule: string | null;
}

export interface TopBlocked {
  domain: string;
  count: number;
}

export const api = {
  toggleProtection: (enable: boolean) =>
    invoke<boolean>("toggle_protection", { enable }),

  getStatus: () => invoke<ProtectionStatus>("get_status"),

  getStats: () => invoke<StatsResponse>("get_stats"),

  getConfig: () => invoke<AppConfig>("get_config"),

  updateConfig: (config: AppConfig) =>
    invoke<void>("update_config", { config }),

  addWhitelist: (domain: string) =>
    invoke<void>("add_whitelist", { domain }),

  removeWhitelist: (domain: string) =>
    invoke<void>("remove_whitelist", { domain }),

  addBlacklist: (domain: string) =>
    invoke<void>("add_blacklist", { domain }),

  removeBlacklist: (domain: string) =>
    invoke<void>("remove_blacklist", { domain }),

  getWhitelist: () => invoke<string[]>("get_whitelist"),

  getBlacklist: () => invoke<string[]>("get_blacklist"),

  getDnsProviders: () => invoke<DnsProvider[]>("get_dns_providers"),

  setDnsProvider: (id: string) =>
    invoke<void>("set_dns_provider", { id }),

  getLogs: (limit?: number) =>
    invoke<QueryEvent[]>("get_logs", { limit: limit ?? 200 }),

  getTopBlocked: () => invoke<TopBlocked[]>("get_top_blocked"),

  updateBlocklists: () => invoke<string>("update_blocklists"),
};
