; ==============================================================================
; NSIS Installation Script for moggyt (x64)
;
; Parameters:
;   /DVERSION=X.Y.Z   (Optional) Application version; defaults to 0.0.0
;   /DOUTFILE=path    (Optional) Target installer path; defaults to <script-dir>\\..\\..\\dist\\moggyt-${VERSION}-x64-setup.exe
; Note: all default on-disk paths anchor on ${__FILEDIR__}, so the script works
; regardless of the directory makensis is invoked from.
; ==============================================================================

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel admin
ManifestDPIAware true

!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"

; ------------------------------------------------------------------------------
; Version & Metadata Definitions
; ------------------------------------------------------------------------------
!ifndef VERSION
  !define VERSION "0.0.0"
!endif

!define PRODUCT_NAME "moggyt"
!define PRODUCT_PUBLISHER "moggyt"
!define PRODUCT_DESCRIPTION "Native YouTube Music client"
!define PRODUCT_EXE "moggyt.exe"
!define PRODUCT_UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_NAME}"
!define PRODUCT_REG_KEY "Software\${PRODUCT_NAME}"

; ------------------------------------------------------------------------------
; General Settings
; ------------------------------------------------------------------------------
Name "${PRODUCT_NAME}"

!ifndef OUTFILE
  !define OUTFILE "${__FILEDIR__}\\..\\..\\dist\\moggyt-${VERSION}-x64-setup.exe"
!endif
OutFile "${OUTFILE}"

InstallDir "$PROGRAMFILES64\moggyt"
InstallDirRegKey HKLM "${PRODUCT_REG_KEY}" "InstallLocation"

; ------------------------------------------------------------------------------
; UI & Page Configuration
; ------------------------------------------------------------------------------
!define MUI_ABORTWARNING

; Start Menu folder tracking variable
Var STARTMENU_FOLDER
Var DesktopCheckbox

; Installer Pages
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY

; Start Menu Page Configuration
!define MUI_STARTMENUPAGE_REGISTRY_ROOT "HKLM"
!define MUI_STARTMENUPAGE_REGISTRY_KEY "${PRODUCT_REG_KEY}"
!define MUI_STARTMENUPAGE_REGISTRY_VALUENAME "StartMenuFolder"
!define MUI_STARTMENUPAGE_DEFAULTFOLDER "${PRODUCT_NAME}"
!insertmacro MUI_PAGE_STARTMENU "Application" $STARTMENU_FOLDER

!insertmacro MUI_PAGE_INSTFILES

; Finish Page Configuration
!define MUI_PAGE_CUSTOMFUNCTION_SHOW FinishShow
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE FinishLeave
!insertmacro MUI_PAGE_FINISH

; Uninstaller Pages
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

; Language Configuration
!insertmacro MUI_LANGUAGE "English"

; ------------------------------------------------------------------------------
; Helper Functions
; ------------------------------------------------------------------------------
Function .onInit
  SetRegView 64
FunctionEnd

Function un.onInit
  SetRegView 64
FunctionEnd

Function FinishShow
  ${NSD_CreateCheckbox} 120u 100u 170u 12u "Create Desktop Shortcut"
  Pop $DesktopCheckbox
  ${NSD_SetState} $DesktopCheckbox ${BST_CHECKED}
FunctionEnd

Function FinishLeave
  ${NSD_GetState} $DesktopCheckbox $0
  ${If} $0 == ${BST_CHECKED}
    SetShellVarContext all
    CreateShortcut "$DESKTOP\${PRODUCT_NAME}.lnk" "$INSTDIR\${PRODUCT_EXE}"
  ${EndIf}
FunctionEnd

; ------------------------------------------------------------------------------
; Installer Sections
; ------------------------------------------------------------------------------
Section "MainSection" SEC01
  SetShellVarContext all
  SetRegView 64

  ; Install application files (path anchored on the script directory)
  SetOutPath "$INSTDIR"
  File "${__FILEDIR__}\\..\\..\\target\\release\\moggyt.exe"

  ; Create uninstaller executable
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  ; Write application registry keys
  WriteRegStr HKLM "${PRODUCT_REG_KEY}" "InstallLocation" "$INSTDIR"

  ; Write Add/Remove Programs registry keys
  WriteRegStr HKLM "${PRODUCT_UNINST_KEY}" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKLM "${PRODUCT_UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${PRODUCT_UNINST_KEY}" "Publisher" "${PRODUCT_PUBLISHER}"
  WriteRegStr HKLM "${PRODUCT_UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${PRODUCT_UNINST_KEY}" "DisplayIcon" "$INSTDIR\${PRODUCT_EXE}"
  WriteRegStr HKLM "${PRODUCT_UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKLM "${PRODUCT_UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${PRODUCT_UNINST_KEY}" "NoRepair" 1

  ; Create Start Menu shortcuts
  !insertmacro MUI_STARTMENU_WRITE_BEGIN Application
    CreateDirectory "$SMPROGRAMS\$STARTMENU_FOLDER"
    CreateShortcut "$SMPROGRAMS\$STARTMENU_FOLDER\${PRODUCT_NAME}.lnk" "$INSTDIR\${PRODUCT_EXE}"
    CreateShortcut "$SMPROGRAMS\$STARTMENU_FOLDER\Uninstall ${PRODUCT_NAME}.lnk" "$INSTDIR\Uninstall.exe"
  !insertmacro MUI_STARTMENU_WRITE_END
SectionEnd

; ------------------------------------------------------------------------------
; Uninstaller Section
; ------------------------------------------------------------------------------
Section "Uninstall"
  SetShellVarContext all
  SetRegView 64

  ; Remove application executable
  Delete "$INSTDIR\${PRODUCT_EXE}"

  ; Remove uninstaller
  Delete "$INSTDIR\Uninstall.exe"

  ; Remove installation directory (only if empty)
  RMDir "$INSTDIR"

  ; Remove Start Menu shortcuts
  !insertmacro MUI_STARTMENU_GETFOLDER Application $STARTMENU_FOLDER
  ${If} $STARTMENU_FOLDER != ""
    Delete "$SMPROGRAMS\$STARTMENU_FOLDER\${PRODUCT_NAME}.lnk"
    Delete "$SMPROGRAMS\$STARTMENU_FOLDER\Uninstall ${PRODUCT_NAME}.lnk"
    RMDir "$SMPROGRAMS\$STARTMENU_FOLDER"
  ${EndIf}

  ; Remove Desktop shortcut
  Delete "$DESKTOP\${PRODUCT_NAME}.lnk"

  ; Remove registry entries
  DeleteRegKey HKLM "${PRODUCT_UNINST_KEY}"
  DeleteRegKey HKLM "${PRODUCT_REG_KEY}"
SectionEnd
