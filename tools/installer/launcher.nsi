; BriskaBlast Launcher — NSIS Installer Template
; NSIS is Zlib licensed (open source, AGPL-3.0 compatible)
; Docs: https://nsis.sourceforge.io/Docs/
;
; TODO before first release:
;   - Set GAME_VERSION from CI (pass via /DGAME_VERSION=x.y.z on makensis command line)
;   - Add icon: replace assets\icon.ico with the actual icon
;   - Verify binary name matches what cargo produces
;   - Add Start Menu shortcut once UX is finalized

!define APP_NAME      "BriskaBlast Launcher"
!define APP_EXE       "briskablast-launcher.exe"
!define INSTALL_DIR   "$PROGRAMFILES64\BriskaBlast\Launcher"
!define OUTPUT_FILE   "BriskaBlast-Launcher-Setup.exe"

Name "${APP_NAME}"
OutFile "../../${OUTPUT_FILE}"
InstallDir "${INSTALL_DIR}"
InstallDirRegKey HKLM "Software\BriskaBlast\Launcher" "InstallDir"
RequestExecutionLevel admin
SetCompressor lzma

; Icon (uncomment when icon exists)
; Icon "..\..\launcher\assets\icon.ico"
; UninstallIcon "..\..\launcher\assets\icon.ico"

Page directory
Page instfiles

Section "Install"
  SetOutPath "$INSTDIR"
  File "..\..\launcher\target\release\${APP_EXE}"

  WriteRegStr HKLM "Software\BriskaBlast\Launcher" "InstallDir" "$INSTDIR"

  ; Start Menu shortcut (uncomment when ready)
  ; CreateDirectory "$SMPROGRAMS\BriskaBlast"
  ; CreateShortcut "$SMPROGRAMS\BriskaBlast\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}"

  ; Add/Remove Programs entry
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\BriskaBlastLauncher" \
    "DisplayName" "${APP_NAME}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\BriskaBlastLauncher" \
    "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  ; RMDir "$SMPROGRAMS\BriskaBlast"
  DeleteRegKey HKLM "Software\BriskaBlast\Launcher"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\BriskaBlastLauncher"
SectionEnd
