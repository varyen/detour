; Установщик Detour для Windows: служба (SYSTEM) + интерфейс (без прав).
; Собирается build.ps1: тот готовит каталог STAGE и передаёт версию.
; Данные (профили, списки, логи) живут в %ProgramData%\Detour и при обновлении
; не трогаются — удалить их предлагается только при деинсталляции.

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef STAGE
  !error "STAGE не задан: путь к каталогу с собранными файлами"
!endif
!ifndef OUTFILE
  !define OUTFILE "detour-setup.exe"
!endif

Name "Detour ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\Detour"
InstallDirRegKey HKLM "Software\Detour" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "Detour"
VIAddVersionKey "FileDescription" "Detour — VPN и обход DPI"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Detour"

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\detour-app.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Открыть Detour"
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Russian"

; Служба держит файлы открытыми: без остановки перезапись не пройдёт.
!macro StopDetour un
Function ${un}StopDetour
  DetailPrint "Останавливаю службу…"
  nsExec::ExecToLog 'net stop DetourSvc'
  Pop $0
  nsExec::ExecToLog 'taskkill /f /im detour-app.exe'
  Pop $0
  nsExec::ExecToLog 'taskkill /f /im sing-box.exe'
  Pop $0
  nsExec::ExecToLog 'taskkill /f /im winws2.exe'
  Pop $0
  Sleep 800
FunctionEnd
!macroend
!insertmacro StopDetour ""
!insertmacro StopDetour "un."

Section "Detour" SecMain
  SectionIn RO
  ; Ярлыки и каталог данных — общие: установщик может работать и от SYSTEM.
  SetShellVarContext all
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "Detour работает только на 64-разрядной Windows."
    Abort
  ${EndIf}

  Call StopDetour
  ; Старая служба могла остаться от прежней версии с другим путём.
  nsExec::ExecToLog '"$INSTDIR\detour-svc.exe" uninstall'
  Pop $0

  SetOutPath "$INSTDIR"
  File "${STAGE}\detour-svc.exe"
  File "${STAGE}\detour-app.exe"
  File "${STAGE}\sing-box.exe"
  ; Движок обхода DPI кладётся, только если он был при сборке: Defender метит
  ; WinDivert, поэтому сборка без него — норма, а поставить его можно из
  ; приложения («Журнал» → «Обновления» → winws2).
!if /FileExists "${STAGE}\winws2.exe"
  File "${STAGE}\winws2.exe"
  File "${STAGE}\cygwin1.dll"
  File "${STAGE}\WinDivert.dll"
  File "${STAGE}\WinDivert64.sys"
  File "${STAGE}\zapret-lib.lua"
  File "${STAGE}\zapret-antidpi.lua"
  File "${STAGE}\zapret-auto.lua"
!endif

  WriteRegStr HKLM "Software\Detour" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Detour" "Version" "${VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Detour" "DisplayName" "Detour"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Detour" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Detour" "Publisher" "Detour"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Detour" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Detour" "DisplayIcon" '"$INSTDIR\detour-app.exe"'
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Detour" "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Detour" "NoRepair" 1
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$SMPROGRAMS\Detour"
  CreateShortcut "$SMPROGRAMS\Detour\Detour.lnk" "$INSTDIR\detour-app.exe"
  CreateShortcut "$SMPROGRAMS\Detour\Удалить Detour.lnk" "$INSTDIR\uninstall.exe"

  ; Интерфейс — окно WebView2. На Windows 11 среда есть всегда, но на чистой
  ; сборке и в песочнице её нет, и окно открывается пустой ошибкой.
  ReadRegStr $0 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  ${If} $0 == ""
    ReadRegStr $0 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" "pv"
  ${EndIf}
  ${If} $0 == ""
    DetailPrint "Ставлю среду WebView2…"
    NSISdl::download /TIMEOUT=60000 "https://go.microsoft.com/fwlink/p/?LinkId=2124703" "$TEMP\MicrosoftEdgeWebview2Setup.exe"
    Pop $1
    ${If} $1 == "success"
      nsExec::ExecToLog '"$TEMP\MicrosoftEdgeWebview2Setup.exe" /silent /install'
      Pop $1
      Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
    ${Else}
      MessageBox MB_ICONEXCLAMATION "Не удалось скачать WebView2 ($1). Поставьте его вручную, иначе окно Detour не откроется."
    ${EndIf}
  ${EndIf}

  DetailPrint "Регистрирую службу DetourSvc…"
  nsExec::ExecToLog '"$INSTDIR\detour-svc.exe" install'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_ICONEXCLAMATION "Служба не зарегистрировалась (код $0). Запустите «$INSTDIR\detour-svc.exe install» от администратора."
  ${EndIf}
SectionEnd

Section "Uninstall"
  ; Данные лежат в ProgramData: в NSIS это $APPDATA при общем контексте.
  SetShellVarContext all
  Call un.StopDetour
  nsExec::ExecToLog '"$INSTDIR\detour-svc.exe" uninstall'
  Pop $0
  ; Правило kill-switch живёт в брандмауэре отдельно от службы: снимаем, иначе
  ; после удаления машина осталась бы без интернета.
  nsExec::ExecToLog 'netsh advfirewall firewall delete rule name="Detour kill-switch"'
  Pop $0

  Delete "$INSTDIR\detour-svc.exe"
  Delete "$INSTDIR\detour-app.exe"
  Delete "$INSTDIR\sing-box.exe"
  Delete "$INSTDIR\winws2.exe"
  Delete "$INSTDIR\cygwin1.dll"
  Delete "$INSTDIR\WinDivert.dll"
  Delete "$INSTDIR\WinDivert64.sys"
  Delete "$INSTDIR\zapret-lib.lua"
  Delete "$INSTDIR\zapret-antidpi.lua"
  Delete "$INSTDIR\zapret-auto.lua"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\Detour\Detour.lnk"
  Delete "$SMPROGRAMS\Detour\Удалить Detour.lnk"
  RMDir "$SMPROGRAMS\Detour"

  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Detour"
  DeleteRegKey HKLM "Software\Detour"

  MessageBox MB_YESNO|MB_ICONQUESTION "Удалить профили, списки и настройки из $APPDATA\Detour?$\n(Они пригодятся, если вы ставите Detour заново.)" IDNO keepdata
    RMDir /r "$APPDATA\Detour"
  keepdata:
SectionEnd
