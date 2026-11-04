; Installer hooks (bundle.windows.nsis.installerHooks in tauri.conf.json).
;
; The admin daemon runs as its own process, "rustatio-daemon.exe", next to the app. Windows does
; not let an installer replace or delete a file that is still in use, and the daemon keeps its log
; file open in the application data folder: stop it before anything is installed or removed, so
; that "Delete the application data" really deletes everything.

!macro RUSTATIO_STOP_DAEMON
  nsExec::Exec 'taskkill /F /T /IM "rustatio-daemon.exe"'
  Pop $0
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro RUSTATIO_STOP_DAEMON
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro RUSTATIO_STOP_DAEMON
!macroend
