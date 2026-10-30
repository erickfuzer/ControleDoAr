; Instalador do Controle do Ar (NSIS 3).
; Gere com installer\build.ps1, que compila o app e passa a versão.
;
; Instala só para o usuário atual (sem administrador), na mesma pasta e com o mesmo registro
; da versão 1, que é substituída.

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma

!include "MUI2.nsh"

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!define APP_NAME "Controle do Ar"
!define EXE "ControleDoAr.exe"
!define PUBLISHER "Erick Fuzer"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"

Name "${APP_NAME}"
OutFile "..\dist\${APP_NAME}_${VERSION}_x64-setup.exe"
InstallDir "$LOCALAPPDATA\${APP_NAME}"
RequestExecutionLevel user
BrandingText "${APP_NAME} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=0 "ProductName" "${APP_NAME}"
VIAddVersionKey /LANG=0 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=0 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=0 "FileDescription" "Instalador do ${APP_NAME}"
VIAddVersionKey /LANG=0 "CompanyName" "${PUBLISHER}"
VIAddVersionKey /LANG=0 "LegalCopyright" "Copyright (c) 2026 Erick Cassol Fuzer. Licença MIT."

!define MUI_ICON "..\assets\icon.ico"
!define MUI_UNICON "..\assets\icon.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"

!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

; O primeiro idioma é o padrão; o NSIS escolhe pelo idioma do Windows.
!insertmacro MUI_LANGUAGE "PortugueseBR"
!insertmacro MUI_LANGUAGE "English"

Section "Install"
  ; Fecha o app se estiver aberto (versão 1 = arcond.exe, versão 2 = ControleDoAr.exe).
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM arcond.exe'
  Pop $0
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM ${EXE}'
  Pop $0
  Sleep 400

  SetOutPath "$INSTDIR"
  File "..\target\release\${EXE}"
  ; Sobras da versão 1 (Tauri): o executável e o perfil do WebView2 (cache e cookies
  ; da janela de login antiga), que a versão 2 não usa.
  Delete "$INSTDIR\arcond.exe"
  RMDir /r "$LOCALAPPDATA\com.erick.arcond"

  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortCut "$SMPROGRAMS\${APP_NAME}.lnk" "$INSTDIR\${EXE}"

  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${EXE}"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1

  ; "Iniciar com o Windows" ligado: passa a apontar para este executável.
  ReadRegStr $0 HKCU "${RUN_KEY}" "${APP_NAME}"
  StrCmp $0 "" +2
    WriteRegStr HKCU "${RUN_KEY}" "${APP_NAME}" '"$INSTDIR\${EXE}"'
SectionEnd

Section "Uninstall"
  nsExec::Exec '"$SYSDIR\taskkill.exe" /F /IM ${EXE}'
  Pop $0
  Sleep 400

  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP_NAME}.lnk"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegValue HKCU "${RUN_KEY}" "${APP_NAME}"

  ; Não deixa login nem configuração para trás.
  nsExec::Exec '"$SYSDIR\cmdkey.exe" /delete:ControleDoAr/smartthings-oauth'
  Pop $0
  nsExec::Exec '"$SYSDIR\cmdkey.exe" /delete:ControleDoAr/smartthings-pat'
  Pop $0
  RMDir /r "$APPDATA\com.erick.arcond"
  RMDir /r "$LOCALAPPDATA\com.erick.arcond"
SectionEnd
