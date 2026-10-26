// Autoescribir el login de Riot (opcional). La contraseña NUNCA va como argumento a PowerShell (sería
// visible para otros procesos): la pone el proceso principal en el portapapeles y aquí solo se mandan
// teclas (Ctrl+V, Tab, Enter) a la ventana del Riot Client. Idea basada en League Accounts (MIT).
const { execFile } = require('child_process');

const WINDOW = 'Riot Client';

function powershell(script, timeout = 15000) {
  return new Promise((resolve) => {
    execFile('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { windowsHide: true, timeout }, (err) =>
      resolve(!err)
    );
  });
}

/** Espera a que exista la ventana del Riot Client (su login). false si no aparece a tiempo. */
async function waitForWindow(timeoutMs = 90_000) {
  const until = Date.now() + timeoutMs;
  const check = `if (Get-Process -Name 'Riot Client' -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle }) { exit 0 } else { exit 1 }`;
  while (Date.now() < until) {
    if (await powershell(check, 8000)) return true;
    await new Promise((r) => setTimeout(r, 1500));
  }
  return false;
}

/**
 * Enfoca la ventana del Riot Client y manda una secuencia de teclas de SendKeys (solo claves fijas,
 * nunca datos: ^a, ^v, {TAB}, {ENTER}). Devuelve false si no pudo enfocarla.
 */
function sendKeys(steps, waitMs = 400) {
  // Las claves son constantes del propio programa; aun así se validan por si acaso.
  if (!steps.every((k) => /^[\^]?[a-z]$|^\{(TAB|ENTER)\}$/.test(k))) return Promise.resolve(false);
  const script = `
$ErrorActionPreference = 'Stop'
$sh = New-Object -ComObject WScript.Shell
if (-not $sh.AppActivate('${WINDOW}')) { exit 2 }
Start-Sleep -Milliseconds ${waitMs}
${steps.map((k) => `$sh.SendKeys('${k}'); Start-Sleep -Milliseconds 150`).join('\n')}
exit 0`;
  return powershell(script);
}

module.exports = { waitForWindow, sendKeys };
