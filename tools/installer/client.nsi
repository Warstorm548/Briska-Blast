; BriskaBlast Client — NSIS Installer Template
; NSIS is Zlib licensed (open source, AGPL-3.0 compatible)
; Docs: https://nsis.sourceforge.io/Docs/
;
; TODO before first release:
;   - Set GAME_VERSION from CI (pass via /DGAME_VERSION=x.y.z on makensis command line)
;   - Add icon: replace assets\icon.ico with the actual icon
;   - Verify binary name matches what cargo produces

!define APP_NAME      "BriskaBlast Client"
!define APP_EXE       "briskablast-client.exe"
!define INSTALL_DIR   "$PROGRAMFILES64\BriskaBlast\Client"
!define OUTPUT_FILE   "BriskaBlast-Client-Setup.exe"

Name "${APP_NAME}"
OutFile "../../${OUTPUT_FILE}"
InstallDir "${INSTALL_DIR}"
InstallDirRegKey HKLM "Software\BriskaBlast\Client" "InstallDir"
RequestExecutionLevel admin
SetCompressor lzma

; Icon (uncomment when icon exists)
; Icon "..\..\client\assets\icon.ico"
; UninstallIcon "..\..\client\assets\icon.ico"

Page directory
Page instfiles

Section "Install"
  SetOutPath "$INSTDIR"
  File "..\..\client\target\release\${APP_EXE}"

  WriteRegStr HKLM "Software\BriskaBlast\Client" "InstallDir" "$INSTDIR"

  ; Add/Remove Programs entry
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\BriskaBlastClient" \
    "DisplayName" "${APP_NAME}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\BriskaBlastClient" \
    "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "Software\BriskaBlast\Client"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\BriskaBlastClient"
SectionEnd
