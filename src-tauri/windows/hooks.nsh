; Uninstall always removes the task-alerts events folder, even when "Delete the application
; data" is left unticked. Claude Code hooks only write when this folder exists, so leftover
; hooks then do nothing instead of collecting prompt text on disk. Kept on updates.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    SetShellVarContext current
    RmDir /r "$APPDATA\${BUNDLEID}\events"
  ${EndIf}
!macroend
