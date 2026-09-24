// Colores personalizables. Cada campo es una variable CSS del tema; los tonos intermedios (paneles más
// claros, bordes, dorado claro/oscuro, texto sobre los botones) se calculan solos a partir de ellos.
// Solo se guardan los colores que el usuario cambió: el resto usa los de styles.css.
// Se carga antes que renderer.js (la app pide los colores apenas arranca), así que no usa sus
// utilidades al cargarse; `toast` solo se llama después, cuando ya existe.
const q = (sel) => document.querySelector(sel);

// [variable, nombre, color predeterminado, dónde se ve]
const THEME_FIELDS = [
  ['bg', 'Fondo de la app', '#080b10', 'El fondo general de todas las pantallas'],
  ['surface', 'Tarjetas y paneles', '#0f141b', 'Tarjetas de cuentas, rangos, diálogos y campos'],
  ['text', 'Texto principal', '#ebe7de', 'Nombres, títulos y la mayoría de los textos'],
  ['muted', 'Texto de ayuda', '#7d8796', 'Textos chicos: explicaciones, fechas, etiquetas'],
  ['gold', 'Botón Jugar y destacados', '#d6b36a', 'Botones principales (Jugar, Nueva cuenta), estrellas de favoritas y la cuenta seleccionada'],
  ['cyan', 'Interruptores y enlaces', '#1fd6d0', 'Interruptores encendidos, enlaces, íconos de datos y el botón Iniciar sesión'],
  ['ok', 'LP ganados y éxito', '#3ccf8e', 'LP ganados, barras de winrate sobre 50%, sesión guardada y mensajes de éxito'],
  ['danger', 'Errores', '#f06a5f', 'Mensajes de error y botones para eliminar'],
  ['win', 'Victorias', '#5b9cf2', 'Victorias en el historial de LoL'],
  ['loss', 'Derrotas y LP perdidos', '#e84057', 'Derrotas en el historial y LP perdidos'],
];
const THEME_DEFAULTS = Object.fromEntries(THEME_FIELDS.map(([k, , v]) => [k, v]));
// Variables que se calculan a partir de las editables.
const THEME_DERIVED = ['surface-2', 'surface-3', 'text-2', 'border', 'border-strong', 'gold-hi', 'gold-lo', 'on-accent', 'on-cyan'];

/** Luminancia relativa (0 negro, 1 blanco) de un color #rrggbb. */
function luminance(hex) {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** Texto legible encima de un color: oscuro sobre colores claros, blanco sobre oscuros. */
function readableOn(hex) {
  return luminance(hex) > 0.35 ? '#15110a' : '#ffffff';
}

function applyTheme(theme = {}) {
  const root = document.documentElement.style;
  for (const [k] of THEME_FIELDS) root.removeProperty(`--${k}`);
  for (const k of THEME_DERIVED) root.removeProperty(`--${k}`);
  for (const [k, v] of Object.entries(theme)) if (k in THEME_DEFAULTS) root.setProperty(`--${k}`, v);

  if (theme.surface || theme.text) {
    root.setProperty('--surface-2', 'color-mix(in srgb, var(--surface) 94%, var(--text))');
    root.setProperty('--surface-3', 'color-mix(in srgb, var(--surface) 88%, var(--text))');
  }
  if (theme.text || theme.bg) {
    root.setProperty('--text-2', 'color-mix(in srgb, var(--text) 76%, var(--bg))');
    root.setProperty('--border', 'color-mix(in srgb, var(--text) 8%, transparent)');
    root.setProperty('--border-strong', 'color-mix(in srgb, var(--text) 15%, transparent)');
  }
  if (theme.gold) {
    root.setProperty('--gold-hi', 'color-mix(in srgb, var(--gold) 72%, white)');
    root.setProperty('--gold-lo', 'color-mix(in srgb, var(--gold) 75%, black)');
    root.setProperty('--on-accent', readableOn(theme.gold));
  }
  if (theme.cyan) root.setProperty('--on-cyan', readableOn(theme.cyan));
}

// ---------- sección "Colores" de Ajustes ----------

let currentTheme = {};
let themeSaveTimer = null;

function renderThemeGrid() {
  q('#themeGrid').innerHTML = THEME_FIELDS.map(
    ([k, label, , tip]) => `<label class="theme-swatch" data-tip="${tip}">
      <input type="color" data-key="${k}" value="${currentTheme[k] || THEME_DEFAULTS[k]}" />
      <span>${label}</span>
    </label>`
  ).join('');
  q('#themeReset').disabled = !Object.keys(currentTheme).length;
}

function saveThemeSoon() {
  clearTimeout(themeSaveTimer);
  themeSaveTimer = setTimeout(() => window.api.setTheme(currentTheme).catch((e) => toast(e.message, 'error')), 400);
}

// Vista previa en vivo mientras se elige el color.
q('#themeGrid').addEventListener('input', (e) => {
  const key = e.target.dataset?.key;
  if (!key) return;
  if (e.target.value.toLowerCase() === THEME_DEFAULTS[key]) delete currentTheme[key];
  else currentTheme[key] = e.target.value.toLowerCase();
  applyTheme(currentTheme);
  q('#themeReset').disabled = !Object.keys(currentTheme).length;
  saveThemeSoon();
});

q('#themeReset').addEventListener('click', () => {
  currentTheme = {};
  applyTheme(currentTheme);
  renderThemeGrid();
  saveThemeSoon();
  toast('Colores predeterminados restablecidos', 'ok');
});

/** Colores guardados en este PC (llegan con el estado de la app, antes de desbloquear). */
function loadTheme(theme) {
  currentTheme = { ...(theme || {}) };
  applyTheme(currentTheme);
}
