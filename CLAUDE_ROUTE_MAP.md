# Claude Route Map

Рекомендуемый набор хостов для маршрута Claude/Anthropic, который нужно вести
через отдельный proxy/VPN-таргет в `route-map.list`.

Пример секции:

```text
// === route:claude_usa_http ===
// meta: strict=1 via_chain=0
claudeusercontent.com
www.claudeusercontent.com
static.claudeusercontent.com
platform.claude.com
api.anthropic.com
console.anthropic.com
statsig.anthropic.com
claude.ai
www.claude.ai
claude.com
www.claude.com
downloads.claude.ai
cdn.claude.ai
assets.claude.ai
160.79.104.0/21
160.79.104.0/23
ipify.org
api.ipify.org
2ip.io
```

Примечания:

- `downloads.claude.ai` нужен для `claude update` и загрузки релизов Claude Code.
- `www.claudeusercontent.com` и `static.claudeusercontent.com` добавлены как
  соседние CDN-hostnames к `claudeusercontent.com`.
- `ipify.org`, `api.ipify.org` и `2ip.io` оставлены в списке как быстрые
  диагностические цели для проверки реального выхода через маршрут.