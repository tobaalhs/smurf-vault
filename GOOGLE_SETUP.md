# Vincular Google Drive (se hace una sola vez, ~10 min)

La app necesita su propio "Client ID" de Google para poder pedirte permiso.
Todo es gratis y solo tu cuenta va a usarlo.

## 1. Crear el proyecto
1. Entra a https://console.cloud.google.com con tu cuenta de Google.
2. Arriba a la izquierda, en el selector de proyectos → **Proyecto nuevo** → nombre `Smurf Vault` → **Crear**.
3. Asegúrate de que quede seleccionado (arriba debe decir `Smurf Vault`).

## 2. Activar la API de Drive
1. Menú ☰ → **APIs y servicios** → **Biblioteca**.
2. Busca **Google Drive API** → **Habilitar**.

## 3. Pantalla de consentimiento (Google Auth Platform)
1. Menú ☰ → **APIs y servicios** → **Pantalla de consentimiento de OAuth** (o "Google Auth Platform") → **Comenzar**.
2. Nombre de la app: `Smurf Vault`. Correo de asistencia: tu Gmail.
3. Público: **Externo**.
4. Correo de contacto: tu Gmail → acepta la política → **Crear**.
5. En **Acceso a los datos** → **Agregar o quitar permisos** → busca y marca:
   - `.../auth/drive.appdata`
   - `openid` y `.../auth/userinfo.email`

   → **Actualizar** → **Guardar**.
6. En **Público** → **Publicar app** → confirmar, para que quede **En producción**.
   > Esto es importante: si la dejas en "Prueba", Google caduca el acceso cada 7 días y tendrías que volver a vincular.
   > Como `drive.appdata` no es un permiso sensible, no te piden verificación.

## 4. Crear las credenciales
1. En **Clientes** → **Crear cliente**.
2. Tipo de aplicación: **App de escritorio**. Nombre: `Smurf Vault desktop` → **Crear**.
3. Aparece un cuadro con el ID. Haz clic en **Descargar JSON**.
4. Renombra el archivo a **`credentials.json`** y déjalo en la carpeta del proyecto (junto a `package.json`).
   Está en el `.gitignore`, así que no se sube a git.

## 5. Vincular desde la app
1. `npm start` → desbloquea la bóveda → ⚙ **Ajustes** → **Vincular cuenta de Google**.
2. Se abre tu navegador. Elige tu cuenta.
3. Google va a decir **"Google no verificó esta app"**. Es normal porque la app es tuya:
   **Configuración avanzada** → **Ir a Smurf Vault (no seguro)** → **Continuar**.
4. **Marca la casilla de Google Drive** ("Ver, crear y borrar sus propios datos de configuración
   en Google Drive"). Google puede mostrarla desmarcada; sin ella la app no puede guardar nada.
5. Aparece "✅ Cuenta de Google vinculada". Vuelve a la app. Arriba debe decir **Drive: sincronizado ✓**.

## ¿Qué se guarda y dónde?
- Un solo archivo `vault.dat` **cifrado** (AES-256 con tu contraseña maestra) en la carpeta
  oculta *appDataFolder* de tu Drive. No lo vas a ver en drive.google.com y la app no puede
  ver el resto de tus archivos.
- Una copia local en `%APPDATA%\Smurf Vault\vault.dat` (y la versión anterior en `vault.prev.dat`).
- Cada vez que guardas algo, se sube a Drive. Al desbloquear, la app usa la versión más nueva
  entre la local y la de Drive.

## En otro PC
Copia el proyecto con `credentials.json`, ejecuta `npm install` y `npm start`. En la pantalla de bloqueo
aprieta **Vincular Google Drive** y la app va a encontrar tu bóveda. Después pones tu contraseña maestra.

> ⚠️ La contraseña maestra no se puede recuperar. Si la olvidas, ni tú ni Google pueden abrir la bóveda.
