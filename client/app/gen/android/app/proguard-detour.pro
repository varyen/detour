# Ядро на Rust зовёт эти методы через JNI — по имени, без ссылок из Java.
-keep class io.github.varyen.detour.DetourBridge { *; }
-keep class io.github.varyen.detour.DetourVpnService { *; }
-keep class io.github.varyen.detour.DetourVpn { *; }
