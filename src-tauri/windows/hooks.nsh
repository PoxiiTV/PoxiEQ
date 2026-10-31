; Al desinstalar PoxiEQ (no en actualizaciones) se quita también su motor de audio, que está oculto en
; «Aplicaciones instaladas»: así no queda un ecualizador funcionando sin forma de verlo.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    SetRegView 64
    ReadRegStr $0 HKLM "SOFTWARE\EqualizerAPO" "InstallPath"
    ${If} $0 != ""
      ExecShellWait "runas" "$INSTDIR\${MAINBINARYNAME}.exe" '--elevated "$TEMP\poxieq-uninstall.json" uninstall' SW_HIDE
    ${EndIf}
  ${EndIf}
!macroend
