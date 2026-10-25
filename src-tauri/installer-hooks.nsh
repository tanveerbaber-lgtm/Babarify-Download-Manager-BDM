; Extra steps for BDM's NSIS installer (Tauri installer hooks).
;
; The browser helper is registered per user by BDM itself on first launch
; (HKCU, no admin needed). Uninstalling removes that registration so Chrome
; and Edge don't keep pointing at a deleted program. Downloaded files are
; never touched.

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\com.babarify.bdm"
  DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\com.babarify.bdm"
  Delete "$APPDATA\com.babarify.bdm\native-host\com.babarify.bdm.json"
  RMDir "$APPDATA\com.babarify.bdm\native-host"
!macroend
