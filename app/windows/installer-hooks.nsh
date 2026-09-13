; NSIS hooks of the Windows installer (tauri.conf.json, bundle > windows >
; nsis > installerHooks), inserted into the installer script of Tauri.
;
; The uninstaller of Tauri removes the files, the shortcuts and the entry of
; "Installed apps", but leaves two things unless "Delete the application
; data" is ticked: the registry key that remembers the installation folder
; (HKCU\Software\<publisher>\4YouPDF, with the language of the installer),
; and the profile of WebView2
; (%LOCALAPPDATA%\org.fouryoupdf.desktop\EBWebView, ${BUNDLEID} being the
; identifier of tauri.conf.json), which holds nothing but the cache of the
; web engine. Neither is data of the user: both go in every case, except
; during an update, which runs the uninstaller of the previous version
; before installing. What the tick box covers beyond that (%APPDATA% and
; %LOCALAPPDATA%\org.fouryoupdf.desktop, where the application writes
; nothing today) still follows it.

!macro NSIS_HOOK_POSTINSTALL
  ; The embedded WebView2 bootstrapper, copied there only when WebView2 was
  ; missing, and run to the end before this point.
  Delete "$TEMP\MicrosoftEdgeWebview2Setup.exe"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
    DeleteRegKey /ifempty SHCTX "${MANUKEY}"
    SetShellVarContext current
    RMDir /r "$LOCALAPPDATA\${BUNDLEID}\EBWebView"
    RMDir "$LOCALAPPDATA\${BUNDLEID}"
  ${EndIf}
!macroend
