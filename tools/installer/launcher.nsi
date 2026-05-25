; BriskaBlast Launcher — NSIS Installer
; NSIS is Zlib licensed (open source, AGPL-3.0 compatible)
; Docs: https://nsis.sourceforge.io/Docs/
;
; CI invokes makensis as:
;   makensis /DVERSION=0.3.0-dev.1 tools/installer/launcher.nsi
; VERSION reaches Add/Remove Programs as `DisplayVersion`; if not provided,
; defaults to 0.0.0 so a hand-run still produces a valid installer.

!ifndef VERSION
  !define VERSION "0.0.0"
!endif

!define APP_NAME      "BriskaBlast Launcher"
!define APP_EXE       "briskablast-launcher.exe"
!define PUBLISHER     "Warstorm548"
!define INSTALL_DIR   "$PROGRAMFILES64\BriskaBlast\Launcher"
!define OUTPUT_FILE   "BriskaBlast-Launcher-Setup.exe"
!define HOME_URL      "https://github.com/Warstorm548/Briska-Blast"
!define ARP_KEY       "Software\Microsoft\Windows\CurrentVersion\Uninstall\BriskaBlastLauncher"

Name "${APP_NAME}"
OutFile "../../${OUTPUT_FILE}"
InstallDir "${INSTALL_DIR}"
InstallDirRegKey HKLM "Software\BriskaBlast\Launcher" "InstallDir"
RequestExecutionLevel admin
SetCompressor lzma

; Branding — installer + uninstaller icon.
Icon "..\..\launcher\assets\icon.ico"
UninstallIcon "..\..\launcher\assets\icon.ico"

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Install"
  SetOutPath "$INSTDIR"
  ; Workspace build — binary is at <repo>\target\release\, not launcher\target\.
  File "..\..\target\release\${APP_EXE}"
  File "..\..\launcher\assets\icon.ico"

  WriteRegStr HKLM "Software\BriskaBlast\Launcher" "InstallDir" "$INSTDIR"

  ; Start Menu shortcut.
  CreateDirectory "$SMPROGRAMS\BriskaBlast"
  CreateShortcut "$SMPROGRAMS\BriskaBlast\${APP_NAME}.lnk" \
    "$INSTDIR\${APP_EXE}" "" "$INSTDIR\icon.ico"

  ; Add/Remove Programs registration.
  WriteRegStr HKLM "${ARP_KEY}" "DisplayName"     "${APP_NAME}"
  WriteRegStr HKLM "${ARP_KEY}" "DisplayVersion"  "${VERSION}"
  WriteRegStr HKLM "${ARP_KEY}" "DisplayIcon"     "$INSTDIR\icon.ico"
  WriteRegStr HKLM "${ARP_KEY}" "Publisher"       "${PUBLISHER}"
  WriteRegStr HKLM "${ARP_KEY}" "URLInfoAbout"    "${HOME_URL}"
  WriteRegStr HKLM "${ARP_KEY}" "InstallLocation" "$INSTDIR"
  ; Quoted — $INSTDIR resolves to "C:\Program Files\BriskaBlast\Launcher"
  ; which has spaces. ARP launches UninstallString verbatim via CreateProcess,
  ; which requires the executable path itself to be quoted.
  WriteRegStr HKLM "${ARP_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKLM "${ARP_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${ARP_KEY}" "NoRepair" 1

  WriteUninstaller "$INSTDIR\Uninstall.exe"
SectionEnd

Section "Uninstall"
  ; --- Launcher binaries (everything this installer wrote) ---
  ; The launcher's writable data (identity.json + saves) now lives under the
  ; per-user data root $APPDATA\BriskaBlast\ (set by the `directories` crate —
  ; see launcher/src/paths.rs), NOT under $INSTDIR. So $INSTDIR holds only the
  ; exe, icon, and uninstaller and `RMDir /r` can clear it cleanly. /r is used
  ; (rather than a bare RMDir) to also sweep an empty legacy `data\` dir left
  ; behind by a pre-migration build. It is scoped strictly to $INSTDIR — it
  ; NEVER touches $APPDATA, which is handled separately and conditionally below.
  RMDir /r "$INSTDIR"
  RMDir "$PROGRAMFILES64\BriskaBlast"

  Delete "$SMPROGRAMS\BriskaBlast\${APP_NAME}.lnk"
  RMDir "$SMPROGRAMS\BriskaBlast"

  DeleteRegKey HKLM "Software\BriskaBlast\Launcher"
  DeleteRegKey HKLM "${ARP_KEY}"

  ; --- Player data prompt ---
  ; The identity file (player_id + secret_token per channel) lives under
  ; $APPDATA\BriskaBlast\. Losing it means losing the account permanently —
  ; there are no cloud saves (see
  ; docs/launcher/launcher-update-and-version-validation.md §Uninstall
  ; Considerations). Default to KEEP: `/SD IDYES` makes silent/unattended
  ; uninstalls preserve data, and the loud branch only deletes on an explicit No.
  MessageBox MB_YESNO|MB_ICONQUESTION \
    "Keep your BriskaBlast player data (account identity and saved games) for a future reinstall?$\n$\nChoose No to permanently delete it. This cannot be undone." \
    /SD IDYES IDYES keep_player_data
    ; User chose No — remove ONLY the per-user data root, nothing outside it.
    RMDir /r "$APPDATA\BriskaBlast"
  keep_player_data:
SectionEnd
