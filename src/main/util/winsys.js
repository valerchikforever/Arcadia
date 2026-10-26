'use strict';
// Windows system access through PowerShell (registry, process list) — no native modules needed.
const { execFile } = require('child_process');

const isWin = process.platform === 'win32';

function runPS(script, timeout = 30000) {
  return new Promise((resolve, reject) => {
    if (!isWin) return resolve('');
    const full = '[Console]::OutputEncoding=[Text.Encoding]::UTF8\n$ProgressPreference="SilentlyContinue"\n' + script;
    const encoded = Buffer.from(full, 'utf16le').toString('base64');
    execFile(
      'powershell.exe',
      ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', encoded],
      { windowsHide: true, maxBuffer: 64 * 1024 * 1024, timeout, encoding: 'utf8' },
      (err, stdout) => {
        if (err && !stdout) return reject(err);
        resolve(String(stdout || '').replace(/^﻿/, ''));
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

module.exports = { runPS, readRegistry, listProcessPaths, isWin };
