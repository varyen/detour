import { requestJson, requestJsonTolerant, requestRawText } from "./client";
import type {
  BypassMode,
  BypassStatus,
  LanClient,
  StatusResponse,
  TrafficLanes,
  UdpVpnMode,
  UdpVpnResponse,
} from "./types";

/* Действия, перестраивающие конфиг, внутри себя спят 2–3 с, а инициализация
   сервиса делает ещё и сброс conntrack с прогревом DNS — реальное время куда
   больше паузы в скрипте. Отсюда щедрые таймауты. */
const RESTART_TIMEOUT = 120_000;

export const overview = {
  status: () => requestJson<StatusResponse>("status", { timeoutMs: 30_000 }),

  lanClients: () =>
    requestJson<{ ok: boolean; clients: LanClient[] }>("lan_clients"),

  /** Дампы nat/nft для раздела «Файрвол». */
  firewall: () => requestJson<{ nat: string; nft: string }>("iptables"),

  /* Таблица соединений. Фильтр — подстрока (адрес, порт); пустой значит «первые
     строки как есть». Роутер отдаёт не больше 200 строк и общее число рядом:
     полный /proc/net/nf_conntrack бывает на десятки тысяч записей. */
  conntrack: (filter?: string) =>
    requestJsonTolerant<{
      conntrack: string;
      total?: number;
      shown?: number;
      filter?: string;
    }>("conntrack", filter ? { params: { filter } } : undefined),

  /* Счётчики по лентам схемы потока. На роутерах со старой прошивкой действия
     ещё нет — тогда приходит {ok:false,"unknown action"}, и схема честно
     показывает прочерки вместо выдуманных долей. */
  traffic: () =>
    requestJsonTolerant<TrafficLanes & { supported?: boolean }>("traffic_counters"),

  allvpnOn: () =>
    requestJson<{ ok: boolean }>("allvpn_on", {
      method: "POST",
      timeoutMs: RESTART_TIMEOUT,
    }),
  allvpnOff: () =>
    requestJson<{ ok: boolean }>("allvpn_off", {
      method: "POST",
      timeoutMs: RESTART_TIMEOUT,
    }),

  bypassStatus: () => requestJsonTolerant<BypassStatus>("bypass_status"),
  bypassSet: (mode: BypassMode) =>
    requestJson<{ ok: boolean; mode: BypassMode }>("bypass_set", {
      method: "POST",
      params: { mode },
      timeoutMs: RESTART_TIMEOUT,
    }),
  bypassStop: () =>
    requestJson<{ ok: boolean }>("bypass_stop", {
      method: "POST",
      timeoutMs: RESTART_TIMEOUT,
    }),
  bypassAutostart: (on: boolean) =>
    requestJson<{ ok: boolean }>("bypass_autostart", {
      method: "POST",
      params: { on: on ? 1 : 0 },
    }),
  /** GET отдаёт голую строку стратегии, а не JSON. */
  bypassStrategyGet: () => requestRawText("bypass_strategy"),
  bypassStrategySet: (line: string) =>
    requestJson<{ ok: boolean }>("bypass_strategy", {
      body: line,
      timeoutMs: RESTART_TIMEOUT,
    }),

  udpVpnGet: () => requestJson<UdpVpnResponse>("udp_vpn"),
  udpVpnSet: (mode: UdpVpnMode) =>
    requestJson<{ ok: boolean }>("udp_vpn", {
      body: { mode },
      timeoutMs: RESTART_TIMEOUT,
    }),
  udpVpnListGet: () => requestJson<{ list: string }>("udp_vpn_list"),
  udpVpnListSet: (list: string) =>
    requestJson<{ ok: boolean }>("udp_vpn_list", {
      body: list,
      timeoutMs: RESTART_TIMEOUT,
    }),
};
