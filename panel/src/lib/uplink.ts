/* Те же подписи, что у detour-wan-link (uplink_label). */
const UPLINK_NAMES: Record<string, string> = {
  wan: "Основной",
  secondwan: "Второй",
  wwan: "Wi-Fi репитер",
  tethering: "USB-модем",
};

export function uplinkName(id: string): string {
  return UPLINK_NAMES[id] ?? (id.startsWith("modem") ? "Мобильный" : id);
}
