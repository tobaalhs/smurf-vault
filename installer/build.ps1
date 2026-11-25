# Arma los archivos de una versión, como electron-builder: instalador, portable y latest.yml (el que
# lee el actualizador: el de Electron y el de esta versión). Quedan en dist-rust\.
#   powershell -File installer\build.ps1            versión para publicar
#   powershell -File installer\build.ps1 -Test      instalador de prueba (otra carpeta y otro GUID)
param([switch]$Test)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
if (-not (Test-Path credentials.json)) { Write-Warning 'Sin credentials.json: Google Drive pedirá cargarlo en Ajustes.' }

cargo build --release -p smurf-vault
if ($LASTEXITCODE) { throw 'Falló la compilación' }
$exe = Join-Path $repo 'target\release\smurf-vault.exe'

$out = Join-Path $repo 'dist-rust'
New-Item -ItemType Directory -Force $out | Out-Null
$suffix = $(if ($Test) { '-test' } else { '' })
$setupName = "Smurf-Vault-Setup-$version$suffix.exe"
$setup = Join-Path $out $setupName

# El NSIS y sus plugins que ya bajó electron-builder.
$nsis = Join-Path $env:LOCALAPPDATA 'electron-builder\Cache\nsis'
$makensis = Join-Path $nsis 'nsis-3.0.4.1\Bin\makensis.exe'
$plugins = Join-Path $nsis 'nsis-resources-3.4.1\plugins\x86-unicode'
if (-not (Test-Path $makensis)) { throw "No encontré makensis en $makensis" }
$defs = @("/DVERSION=$version", "/DEXE=$exe", "/DOUTFILE=$setup", "/X!addplugindir /x86-unicode `"$plugins`"")
if ($Test) { $defs += '/DTEST' }
& $makensis /V2 @defs (Join-Path $PSScriptRoot 'smurf-vault.nsi')
if ($LASTEXITCODE) { throw 'Falló el instalador' }

if (-not $Test) {
  Copy-Item $exe (Join-Path $out "Smurf-Vault-Portable-$version.exe") -Force
  $bytes = [IO.File]::ReadAllBytes($setup)
  $sha = [Convert]::ToBase64String([Security.Cryptography.SHA512]::Create().ComputeHash($bytes))
  $date = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
  $yml = "version: $version`nfiles:`n  - url: $setupName`n    sha512: $sha`n    size: $($bytes.Length)`npath: $setupName`nsha512: $sha`nreleaseDate: '$date'`n"
  [IO.File]::WriteAllText((Join-Path $out 'latest.yml'), $yml, (New-Object Text.UTF8Encoding $false))
}
Get-ChildItem $out | Select-Object Name, @{ n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) } }
