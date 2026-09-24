package io.github.varyen.detour

import android.Manifest
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import java.io.File

class MainActivity : TauriActivity() {
  private val vpnConsent = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
    asking = false
  }
  private val notifications = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

  /** Запрос показывает системный диалог; второй такой же только мешает. */
  private var asking = false

  /** Отступы системных полос в CSS-пикселях, JSON для панели. */
  @Volatile private var insetsJson = """{"top":0,"bottom":0,"left":0,"right":0}"""

  override fun onCreate(savedInstanceState: Bundle?) {
    // До super: ядро стартует уже внутри него и к этому моменту должно знать,
    // куда звать за туннелем и где лежит движок обхода DPI.
    DetourVpn.attach(this)
    DetourVpn.askPermission = { runOnUiThread { requestVpn() } }
    DetourBridge.nativeInit(File(applicationInfo.nativeLibraryDir, "libtpws.so").absolutePath)
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    requestVpn()
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      runCatching { notifications.launch(Manifest.permission.POST_NOTIFICATIONS) }
    }
  }

  // Окно рисуется под статусбаром и навбаром (edge-to-edge), а WebView до
  // Chromium 136 не отдаёт env(safe-area-inset-*) — без этого шапка панели
  // уезжает под шторку и её кнопки не нажать. Отступы уходят в CSS-переменные.
  override fun onWebViewCreate(webView: WebView) {
    webView.addJavascriptInterface(Bridge(), "DetourInsets")
    ViewCompat.setOnApplyWindowInsetsListener(webView) { v, insets ->
      val b = insets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
      val d = resources.displayMetrics.density
      insetsJson = """{"top":${b.top / d},"bottom":${b.bottom / d},"left":${b.left / d},"right":${b.right / d}}"""
      v.post { webView.evaluateJavascript("window.__detourInsets&&window.__detourInsets($insetsJson)", null) }
      insets
    }
    ViewCompat.requestApplyInsets(webView)
  }

  private inner class Bridge {
    @JavascriptInterface
    fun insets(): String = insetsJson

    @JavascriptInterface
    fun theme(dark: Boolean) {
      runOnUiThread {
        WindowCompat.getInsetsController(window, window.decorView).apply {
          isAppearanceLightStatusBars = !dark
          isAppearanceLightNavigationBars = !dark
        }
      }
    }
  }

  private fun requestVpn() {
    if (asking) return
    VpnService.prepare(this)?.let {
      asking = true
      runCatching { vpnConsent.launch(it) }.onFailure { asking = false }
    }
  }
}
