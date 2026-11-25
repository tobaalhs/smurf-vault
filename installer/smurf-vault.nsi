; Instalador de Smurf Vault (versión Rust). Reemplaza al de electron-builder sin que el usuario
; haga nada: misma carpeta (%LOCALAPPDATA%\Programs\Smurf Vault), misma entrada en "Agregar o quitar
; programas" (mismo GUID), mismos accesos directos y mismo ejecutable ("Smurf Vault.exe"). Así la
; versión Electron se actualiza sola a esta con su electron-updater, que lo abre con:
;   --updated            es una actualización (no se pregunta la carpeta)
;   /S                   sin ventanas (al cerrar la app)
;   --force-run          volver a abrir la app al terminar
; La bóveda y los ajustes están en %APPDATA%\Smurf Vault y no se tocan.
;
; makensis /DVERSION=0.9.0 /DEXE=ruta\smurf-vault.exe /DOUTFILE=... smurf-vault.nsi
; /DTEST usa otro GUID y otra carpeta, para probar sin tocar la instalación real.

Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!ifndef VERSION
  !error "Falta /DVERSION"
!endif

!define PRODUCT "Smurf Vault"
!define APP_ID "com.tobaalhs.smurfvault"
!define PUBLISHER "tobaalhs"
!ifdef TEST
  ; Prueba: otro GUID, otra carpeta y otros accesos directos; no cierra la app real ni toca el
  ; inicio con Windows.
  !define GUID "2360793d-0000-0000-0000-00000000test"
  !define DIRNAME "Smurf Vault Test"
  !define LINK "Smurf Vault Test"
!else
  ; El GUID que electron-builder sacó del appId: el de la instalación que ya existe.
  !define GUID "2360793d-28e1-524e-9884-ff6968fae7cf"
  !define DIRNAME "Smurf Vault"
  !define LINK "Smurf Vault"
!endif
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${GUID}"
!define APP_KEY "Software\${GUID}"
!define EXE_NAME "Smurf Vault.exe"
!define UNINSTALLER "Uninstall Smurf Vault.exe"

Name "${PRODUCT}"
OutFile "${OUTFILE}"
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\Programs\${DIRNAME}"
InstallDirRegKey HKCU "${APP_KEY}" "InstallLocation"
SetCompressor /SOLID lzma
BrandingText "${PRODUCT} ${VERSION}"
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "FileDescription" "Instalador de ${PRODUCT}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "${PUBLISHER}"

!define MUI_ICON "icon.ico"
!define MUI_UNICON "icon.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE_NAME}"
!define MUI_FINISHPAGE_RUN_TEXT "Abrir ${PRODUCT}"

Var Updated
Var ForceRun

!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipDirIfUpdated
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipFinishIfUpdated
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Spanish"

Function .onInit
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "--updated" $R1
  ${IfNot} ${Errors}
    StrCpy $Updated "1"
  ${EndIf}
  ClearErrors
  ${GetOptions} $R0 "--force-run" $R1
  ${IfNot} ${Errors}
    StrCpy $ForceRun "1"
  ${EndIf}
FunctionEnd

Function SkipDirIfUpdated
  ${If} $Updated == "1"
    Abort
  ${EndIf}
FunctionEnd

Function SkipFinishIfUpdated
  ; En una actualización se vuelve a abrir sola (abajo); no hace falta la última página.
  ${If} $Updated == "1"
    Abort
  ${EndIf}
FunctionEnd

; Cierra la app si está abierta (Electron o Rust: el ejecutable se llama igual) y espera.
!macro CloseApp
  !ifndef TEST
    nsExec::Exec 'taskkill /F /IM "${EXE_NAME}"'
    Pop $0
    Sleep 800
  !endif
!macroend

Section "Smurf Vault" SecMain
  !insertmacro CloseApp
  SetOutPath "$INSTDIR"

  ; Restos de la versión Electron (Chromium): ya no se usan.
  RMDir /r "$INSTDIR\resources"
  RMDir /r "$INSTDIR\locales"
  RMDir /r "$INSTDIR\swiftshader"
  Delete "$INSTDIR\LICENSE.electron.txt"
  Delete "$INSTDIR\LICENSES.chromium.html"
  Delete "$INSTDIR\chrome_100_percent.pak"
  Delete "$INSTDIR\chrome_200_percent.pak"
  Delete "$INSTDIR\resources.pak"
  Delete "$INSTDIR\d3dcompiler_47.dll"
  Delete "$INSTDIR\ffmpeg.dll"
  Delete "$INSTDIR\libEGL.dll"
  Delete "$INSTDIR\libGLESv2.dll"
  Delete "$INSTDIR\vk_swiftshader.dll"
  Delete "$INSTDIR\vk_swiftshader_icd.json"
  Delete "$INSTDIR\vulkan-1.dll"
  Delete "$INSTDIR\icudtl.dat"
  Delete "$INSTDIR\snapshot_blob.bin"
  Delete "$INSTDIR\v8_context_snapshot.bin"
  Delete "$INSTDIR\version"

  File "/oname=${EXE_NAME}" "${EXE}"
  WriteUninstaller "$INSTDIR\${UNINSTALLER}"

  ; Accesos directos con el id de la app (para los avisos de Windows). En una actualización solo se
  ; rehace el del escritorio si ya existía.
  CreateShortCut "$SMPROGRAMS\${LINK}.lnk" "$INSTDIR\${EXE_NAME}"
  WinShell::SetLnkAUMI "$SMPROGRAMS\${LINK}.lnk" "${APP_ID}"
  ${If} $Updated != "1"
  ${OrIf} ${FileExists} "$DESKTOP\${LINK}.lnk"
    CreateShortCut "$DESKTOP\${LINK}.lnk" "$INSTDIR\${EXE_NAME}"
    WinShell::SetLnkAUMI "$DESKTOP\${LINK}.lnk" "${APP_ID}"
  ${EndIf}

  ; "Agregar o quitar programas": la misma entrada que dejó electron-builder.
  WriteRegStr HKCU "${APP_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${APP_KEY}" "ShortcutName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT} ${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${EXE_NAME},0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\${UNINSTALLER}" /currentuser'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\${UNINSTALLER}" /currentuser /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" "$0"

  ; Abrir al iniciar Windows: si estaba activado, se deja apuntando a este ejecutable.
  !ifndef TEST
    ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCT}"
    ${If} $0 != ""
      WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCT}" '"$INSTDIR\${EXE_NAME}" --hidden'
    ${EndIf}
  !endif

  ; Actualización: se vuelve a abrir sola si lo pidieron (--force-run).
  ${If} $ForceRun == "1"
    Exec '"$INSTDIR\${EXE_NAME}"'
  ${EndIf}
SectionEnd

Section "Uninstall"
  !insertmacro CloseApp
  Delete "$INSTDIR\${EXE_NAME}"
  Delete "$INSTDIR\${UNINSTALLER}"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${LINK}.lnk"
  Delete "$DESKTOP\${LINK}.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "${APP_KEY}"
  !ifndef TEST
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCT}"
  !endif
  ; La bóveda (%APPDATA%\Smurf Vault) no se borra: es tuya y puede estar solo en este PC.
SectionEnd
