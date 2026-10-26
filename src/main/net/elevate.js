'use strict';
// Running things with administrator rights (UAC prompt) and checking running processes by name.
const { execFile } = require('child_process');
const fs = require('fs');
const { runPS, writeTempScript, isWin } = require('../util/winsys');

const psq = (s) => `'${String(s).replace(/'/g, "''")}'`;

// Runs a PowerShell script in an elevated, hidden PowerShell. Rejects if the user cancels the UAC prompt.
async function runElevated(innerScript, { wait = false } = {}) {
  if (!isWin) throw new Error('Доступно только в Windows');
  // The elevated script lives in a temp file and removes itself when done
  const file = writeTempScript(`$ErrorActionPreference="SilentlyContinue"
try {
${innerScript}
} finally { Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue }`, 'arcadia-admin');
  const fileArg = '"' + file + '"';
  const outer = `
try {
  Start-Process -FilePath 'powershell.exe' -Verb RunAs -WindowStyle Hidden ${wait ? '-Wait' : ''} -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',${psq(fileArg)}) -ErrorAction Stop
  Write-Output 'OK'
} catch { Write-Output ('ERR:' + $_.Exception.Message) }`;
  let out;
  try { out = (await runPS(outer, wait ? 120000 : 60000)).trim(); } catch (e) { fs.rm(file, { force: true }, () => {}); throw e; }
  if (out.startsWith('ERR:')) {
    fs.rm(file, { force: true }, () => {});
    const msg = out.slice(4);
    throw new Error(/cancel|отмен/i.test(msg) ? 'Запрос прав администратора отклонён' : msg);
  }
  return true;
}

// Lower-cased image names of all running processes (no admin rights needed)
function processNames() {
  return new Promise((resolve) => {
    if (!isWin) return resolve(new Set());
    execFile('tasklist.exe', ['/FO', 'CSV', '/NH'], { windowsHide: true, maxBuffer: 16 * 1024 * 1024 }, (err, out) => {
      if (err) return resolve(new Set());
      const set = new Set();
      for (const line of String(out).split(/\r?\n/)) {
        const m = /^"([^"]+)"/.exec(line);
        if (m) set.add(m[1].toLowerCase());
      }
      resolve(set);
    });
  });
}

module.exports = { runElevated, processNames, psq };
