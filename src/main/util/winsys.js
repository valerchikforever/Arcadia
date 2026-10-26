'use strict';
// Windows system access through PowerShell (registry, process list) — no native modules needed.
const { execFile } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const isWin = process.platform === 'win32';

// Scripts go through a temporary .ps1 file: passing them on the command line
// hits the 32 767-character limit of Windows (spawn ENAMETOOLONG) for long zapret strategies.
let psCounter = 0;
function writeTempScript(script, prefix = 'arcadia') {
  const file = path.join(os.tmpdir(), `${prefix}-${process.pid}-${Date.now()}-${psCounter++}.ps1`);
  // UTF-8 with BOM, otherwise Windows PowerShell 5.1 misreads Cyrillic paths
  fs.writeFileSync(file, '\uFEFF' + script, 'utf8');
  return file;
}

function runPS(script, timeout = 30000) {
  return new Promise((resolve, reject) => {
    if (!isWin) return resolve('');
    const full = '[Console]::OutputEncoding=[Text.Encoding]::UTF8\n$ProgressPreference="SilentlyContinue"\n' + script;
    let file;
    try { file = writeTempScript(full); } catch (e) { return reject(e); }
    execFile(
      'powershell.exe',
      ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', file],
      { windowsHide: true, maxBuffer: 64 * 1024 * 1024, timeout, encoding: 'utf8' },
      (err, stdout) => {
        fs.rm(file, { force: true }, () => {});
        if (err && !stdout) return reject(err);
        resolve(String(stdout || '').replace(/^\uFEFF/, ''));
      }
    );
  });
}

// Read registry keys (their own values + direct subkeys with values).
// Returns { 'HKEY_...\\Path': { values: {...}, subkeys: [{ name, values }] } }
async function readRegistry(keys) {
  if (!isWin) return {};
  const list = keys.map((k) => `'${k.replace(/'/g, "''")}'`).join(',');
  const script = `
$keys=@(${list})
$res=@{}
foreach($p in $keys){
  $full='Registry::'+$p
  if(-not (Test-Path -LiteralPath $full)){ continue }
  $k=Get-Item -LiteralPath $full -ErrorAction SilentlyContinue
  if(-not $k){ continue }
  $vals=@{}
  foreach($n in $k.GetValueNames()){ try { $vals[$n]=[string]$k.GetValue($n) } catch {} }
  $subs=@()
  foreach($c in (Get-ChildItem -LiteralPath $full -ErrorAction SilentlyContinue)){
    $sv=@{}
    foreach($n in $c.GetValueNames()){ try { $sv[$n]=[string]$c.GetValue($n) } catch {} }
    $subs+=@{name=$c.PSChildName; values=$sv}
  }
  $res[$p]=@{values=$vals; subkeys=$subs}
}
ConvertTo-Json -InputObject $res -Depth 6 -Compress`;
  const out = await runPS(script, 60000);
  const txt = out.trim();
  if (!txt) return {};
  const data = JSON.parse(txt);
  for (const k of Object.keys(data)) {
    const e = data[k];
    if (e && e.subkeys && !Array.isArray(e.subkeys)) e.subkeys = [e.subkeys];
  }
  return data;
}

// Full paths of every running process executable
async function listProcessPaths() {
  if (!isWin) return [];
  const out = await runPS(
    "Get-CimInstance Win32_Process -Property ExecutablePath | ForEach-Object { $_.ExecutablePath } | Where-Object { $_ }",
    20000
  );
  return out.split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
}

module.exports = { writeTempScript, runPS, readRegistry, listProcessPaths, isWin };
