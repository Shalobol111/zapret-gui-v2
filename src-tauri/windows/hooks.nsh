; ZapretGUI installer hooks (Tauri NSIS).
; On uninstall remove the autostart task created by the app (Settings > "Run with Windows").
; The zapret service and the zapret folder belong to zapret itself and are left untouched.

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec 'schtasks /delete /tn "ZapretGUI_Autostart" /f'
  Pop $0
!macroend
