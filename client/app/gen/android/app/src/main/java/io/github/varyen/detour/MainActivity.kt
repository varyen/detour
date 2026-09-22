package io.github.varyen.detour

import android.Manifest
import android.net.VpnService
import android.os.Build
import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import java.io.File

class MainActivity : TauriActivity() {
  private val vpnConsent = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
    asking = false
  }
  private val notifications = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

  /** Запрос показывает системный диалог; второй такой же только мешает. */
  private var asking = false

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

  private fun requestVpn() {
    if (asking) return
    VpnService.prepare(this)?.let {
      asking = true
      runCatching { vpnConsent.launch(it) }.onFailure { asking = false }
    }
  }
}
