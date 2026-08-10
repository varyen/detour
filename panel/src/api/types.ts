/* Формы ответов detour-api. Описано то, что панель действительно читает;
   поля, которые бэкенд отдаёт «на всякий случай», намеренно не тянем. */

export type Platform = "openwrt" | "keenetic";
export type RoutingMode = "proxy-list" | "all-except";
export type SingboxMode = "single" | "multi";
export type BypassMode = "off" | "zapret" | "zapret2";
export type UdpVpnMode = "off" | "list" | "all";
export type HealthState = "ok" | "slow" | "dead" | "unknown";

export interface Binaries {
  bins_version?: string;
  singbox_version?: string;
  singbox_present?: boolean;
  tpws_present?: boolean;
  tpws_version?: string;
  nfqws2_present?: boolean;
  nfqws2_version?: string;
  nfqws2_supported?: boolean;
}

export interface SingboxStatus {
  running: boolean;
  pid?: number | string;
  port?: number;
  enabled?: boolean;
  allvpn?: boolean;
  domains?: number;
  ips?: number;
  entries?: number;
  ipset_count?: number;
  ipset_members?: number;
  active_profile?: string;
  active_type?: string;
  external_ip?: string;
  external_ip_checked?: number;
  external_ip_refreshing?: boolean;
  active_chain?: string[];
  routing_mode?: RoutingMode;
  singbox_mode?: SingboxMode;
  route_targets?: number;
  self_intercept?: string[];
}

export interface ZapretStatus {
  running: boolean;
  pid?: number | string;
  port?: number;
  enabled?: boolean;
  domains?: number;
  ips?: number;
  ipset_count?: number;
  args?: string;
}

export interface SystemStatus {
  mptcp?: string | number;
  uptime?: string;
  memory?: string;
  disk_free?: string;
  cpu?: string;
  cpu_cores?: number;
}

export interface WanLink {
  ok?: boolean;
  iface?: string;
  speed?: number | string;
  duplex?: string;
  warn?: string;
}

export interface StatusResponse {
  platform: Platform;
  panel_port?: number;
  version?: string;
  binaries: Binaries;
  singbox: SingboxStatus;
  zapret: ZapretStatus;
  system: SystemStatus;
  wan_link?: WanLink;
}

export interface ProfileSummary {
  id: string;
  type: string;
  name: string;
  group?: string;
  routing_mode?: string;
  autoswitch?: boolean;
  speedcheck?: boolean;
}

export interface ProfilesListResponse {
  profiles: ProfileSummary[];
  active?: string;
  active_chain?: string[];
}

export interface Chain {
  id: string;
  name: string;
  hops: string[];
}

export interface ChainsResponse {
  chains: Chain[];
}

export interface PingResult {
  rtt?: number;
  ok?: boolean;
  ts?: number;
  server?: string;
  method?: "icmp" | "tcp" | "none";
}

export interface PingStatusResponse {
  now: number;
  results: Record<string, PingResult>;
}

export interface HealthResult {
  ok?: boolean;
  ts?: number;
  rtt?: number;
  dl?: number;
  delays?: number[];
}

export interface HealthStatusResponse {
  now: number;
  supported?: boolean;
  enabled?: boolean;
  auto_switch?: boolean;
  speed?: boolean;
  speed_bytes?: number;
  urls?: { label: string; url: string }[];
  switch?: unknown;
  results: Record<string, HealthResult>;
}

export interface BypassStatus {
  mode: BypassMode;
  autostart?: boolean;
  running?: boolean;
  zapret2_supported?: boolean;
  platform?: Platform;
}

export interface UdpVpnResponse {
  mode: UdpVpnMode;
  supported: boolean;
  list?: string;
}

export interface UpdateChannelState {
  update_available?: boolean;
  available_version?: string;
  current_version?: string;
  upstream?: string;
  upstream_newer?: boolean;
  changelog_b64?: string;
  checked?: number;
  error?: string;
}

export interface UpdatesOverview {
  panel?: UpdateChannelState;
  singbox?: UpdateChannelState;
  tpws?: UpdateChannelState;
  nfqws2?: UpdateChannelState;
}

export interface ApplyLogResponse {
  ok?: boolean;
  done?: boolean;
  rc?: number;
  log?: string;
}

/**
 * Подписка так, как её хранит роутер: один файл на подписку в
 * /etc/detour/subscriptions/<id>.json. Имена полей — те, что реально пишет и
 * читает CGI (title/autoupdate/interval_hours), а не привычные name/enabled.
 * Панель обязана сохранять и незнакомые поля записи (last_*), иначе правка в
 * форме затрёт то, что проставил refresh.
 */
export interface Subscription {
  id: string;
  url: string;
  title?: string;
  group?: string;
  autoupdate?: boolean;
  interval_hours?: number;
  apply_routing?: boolean;
  user_agent?: string;
  user_agent_preset?: string;
  last_refresh?: number;
  last_status?: string;
  last_error?: string;
  last_saved?: number;
  last_removed?: number;
  /** Старые написания: встречаются в записях, сделанных прежней панелью. */
  name?: string;
  enabled?: boolean;
  interval?: number;
  count?: number;
}

export interface PortmapEntry {
  id: string;
  name?: string;
  enabled: boolean;
  mode: "https" | "dnat";
  listen_port: number;
  proto: string;
  target_ip: string;
  target_port: number;
  scheme?: string;
  src?: string;
  user?: string;
}

export interface PortmapStatus {
  ok: boolean;
  platform?: Platform;
  engine?: string;
  https_supported?: boolean;
  https_reason?: string;
  dnat_supported?: boolean;
  dnat_reason?: string;
  entries?: PortmapEntry[];
}

export interface CertStatus {
  ok?: boolean;
  domain?: string;
  email?: string;
  expiry?: string;
  state?: string;
  running?: boolean;
  error?: string;
  log?: string;
}

export interface WarpStatus {
  ok: boolean;
  supported: boolean;
  profiles?: { id: string; name?: string; verified?: boolean }[];
  last?: { state?: string; error?: string; ts?: number };
}

export interface SwapStatus {
  ok: boolean;
  supported: boolean;
  exists?: boolean;
  active?: boolean;
  size_mb?: number;
  free_mb?: number;
  swappiness?: number;
  tools?: boolean;
  busy?: boolean;
  last_result?: string;
  last_message?: string;
}

export interface OffloadStatus {
  ok: boolean;
  supported: boolean;
  mode?: "auto" | "notify" | "off";
  state?: string;
}

export interface RulistStatus {
  ok?: boolean;
  supported: boolean;
  source?: "maxmind" | "rir";
  auto?: boolean;
  enabled?: boolean;
  count?: number;
  updated?: number;
}

export interface HostsStatus {
  ok?: boolean;
  enabled?: boolean;
  url?: string;
  count?: number;
  updated?: number;
  exclude_enabled?: boolean;
  custom_enabled?: boolean;
  secure_dns_mode?: string;
  secure_dns_list?: string;
}

export interface PushConfig {
  ok: boolean;
  available: boolean;
  vapid_public?: string;
  sub_count?: number;
  bypass?: boolean;
}

export interface LanClient {
  ip: string;
  host?: string;
  mac?: string;
}

/** Ленты схемы потока. Пока считаются по conntrack, позже — по счётчикам nft. */
export interface TrafficLanes {
  direct: number;
  vpn: number;
  bypass: number;
  total: number;
  /** true — точные счётчики файрвола, false — оценка по числу соединений. */
  exact: boolean;
}
