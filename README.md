# Smurf Vault

Bóveda cifrada para tus cuentas de League of Legends, con copia en Google Drive.

- 🔐 Todo cifrado con tu contraseña maestra (scrypt + AES-256-GCM).
- ☁️ Sincroniza con tu Google Drive → [GOOGLE_SETUP.md](GOOGLE_SETUP.md)
- 🎮 **Detectar cliente**: con el LoL abierto, lee nick, tag, nivel, ícono y rango de la cuenta en la que
  iniciaste sesión (API local del cliente). Revisa cada 30 s y actualiza sola las cuentas ya vinculadas.
- 📈 **Actualizar rangos**: usa la API oficial de Riot para refrescar todas las cuentas sin abrir el juego.
- 📋 Copiar usuario y contraseña con un clic. El portapapeles se limpia a los 30 s.
- 📥 **Importar**: pega tu txt viejo (`usuario:contraseña` por línea).

## Uso
```bash
npm install
npm start
```

## API key de Riot (opcional, para "Actualizar rangos")
1. Entra a https://developer.riotgames.com con tu cuenta de Riot.
2. Copia la **Development API Key**. Dura 24 h y sirve para probar.
3. Para que no caduque: **Register Product** → **Personal API Key**. Describe que es una app personal para ver
   el rango de tus propias cuentas. Suelen aprobarla en unos días.
4. Pégala en ⚙ Ajustes. Queda guardada dentro de la bóveda cifrada.

Para buscar una cuenta, la API necesita el PUUID (lo obtiene al detectarla con el cliente) o el Riot ID
`Nombre#TAG` escrito en la cuenta. El PUUID no cambia aunque cambies el nick.

## Estructura
- `main.js`: proceso principal: bóveda en memoria, IPC, sincronización.
- `src/vault.js`: cifrado.
- `src/drive.js`: OAuth de Google (loopback + PKCE) y archivo en `appDataFolder`.
- `src/lcu.js`: API local del cliente de LoL.
- `src/riot.js`: API oficial de Riot.
- `renderer/`: interfaz.
