# tools/

Вспомогательное для разработки плагина: стенды, генераторы, проверки. В пакет
не попадает — `build_plugin.py` берёт из плагина только `plugin.json`, `api`,
`panel/`, `cgi-bin/` и то, что перечислено в `files`.

Локальная проверка обработчика без роутера:

```sh
PLUGIN_DATA=/tmp/hello PLUGIN_SETTINGS=/tmp/hello/settings.json \
PLUGIN_PLATFORM=openwrt PLUGIN_METHOD=POST sh plugins/hello/api card
```
