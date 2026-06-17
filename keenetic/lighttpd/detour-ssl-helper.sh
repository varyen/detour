#!/opt/bin/sh
# Emit the optional TLS overlay for the Detour panel's lighttpd, or nothing.
#
# Why a script and not an inline `include_shell "cat … 2>/dev/null || :"`:
# lighttpd runs a "simple" include_shell command DIRECTLY via execve (no shell)
# when the string has no metacharacters it recognises. In that path `2>/dev/null`,
# `||` and `:` are handed to `cat` as literal filenames, cat fails (exit 1), and
# lighttpd treats a non-zero include_shell exit as a FATAL config error — the panel
# never starts ("panel stopped"). Pointing include_shell at this single bare path
# sidesteps that: whether lighttpd execve's it or runs it via /bin/sh -c, the kernel
# honours the shebang and a real shell processes the redirect and `|| true` below,
# so the command always exits 0 — even when the cert overlay does not exist yet.
cat /opt/etc/lighttpd/conf.d/detour-ssl.conf 2>/dev/null || true
