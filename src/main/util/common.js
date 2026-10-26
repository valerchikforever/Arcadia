'use strict';
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

// Comparison key for paths (case-insensitive, slash-agnostic)
function normPath(p) {
  if (!p) return '';
  return String(p).replace(/[\\/]+/g, '/').replace(/\/+$/, '').toLowerCase();
}

function isInside(child, parent) {
  const c = normPath(child), p = normPath(parent);
  return !!p && (c === p || c.startsWith(p + '/'));
}

function exists(p) {
  try { return !!p && fs.existsSync(p); } catch { return false; }
}

function isDir(p) {
  try { return fs.statSync(p).isDirectory(); } catch { return false; }
}

function hashId(s) {
  return crypto.createHash('sha1').update(String(s)).digest('hex').slice(0, 16);
}

// Normalize Windows-style paths coming from registries / manifests
function fixPath(p) {
  if (!p) return '';
  let s = String(p).trim().replace(/^"+|"+$/g, '');
  if (process.platform === 'win32') s = path.win32.normalize(s);
  return s.replace(/[\\/]+$/, '') || s;
}

const SCENE_TAGS = [
  'repack', 'fitgirl', 'dodi', 'xatab', 'mechanics', 'механики', 'r\\.?g\\.?', 'elamigos', 'steamrip',
  'codex', 'plaza', 'skidrow', 'empress', 'flt', 'tenoke', 'rune', 'razor1911', 'goldberg', 'portable',
  'multi\\d*', 'eng', 'rus', 'ru', 'en', 'by\\s+\\S+', 'gog', 'drm[- ]?free', 'p2p', 'kaos', 'decepticon',
  'chovka', 'catalyst', 'online-fix', 'onlinefix', 'repack by \\S+', 'lossless'
];
const TAG_RE = new RegExp(`(?<![\\p{L}\\d])(?:${SCENE_TAGS.join('|')})(?![\\p{L}\\d])`, 'giu');

// Turn "Cyberpunk.2077.v2.1-FitGirl" / "Hollow Knight [GOG] (v1.5)" into a readable name
function cleanName(raw) {
  if (!raw) return '';
  let s = String(raw);
  const sceneStyle = !/\s/.test(s) && /[._]/.test(s);
  s = s.replace(/\[[^\]]*\]|\([^)]*\)|\{[^}]*\}/g, ' ');
  if (sceneStyle) s = s.replace(/-[A-Za-z0-9]+$/, '');
  s = s.replace(/(?<![\p{L}\d])(?:v|ver|version|build|b)\s?\.?\d[\w.\-]*/giu, ' ');
  s = sceneStyle ? s.replace(/[._]+/g, ' ') : s.replace(/_+/g, ' ');
  s = s.replace(TAG_RE, ' ');
  s = s.replace(/\s+[-–—+&|]+\s*$/g, '').replace(/^\s*[-–—+&|]+\s+/g, '');
  s = s.replace(/\s{2,}/g, ' ').trim();
  return s || String(raw).trim();
}

// Loose normalization for matching names between sources
function matchKey(s) {
  return String(s || '')
    .toLowerCase()
    .replace(/[™®©:’'`"!?,.\-–—_()\[\]]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

function similarity(a, b) {
  const ta = new Set(matchKey(a).split(' ').filter(Boolean));
  const tb = new Set(matchKey(b).split(' ').filter(Boolean));
  if (!ta.size || !tb.size) return 0;
  let inter = 0;
  for (const t of ta) if (tb.has(t)) inter++;
  return inter / (ta.size + tb.size - inter);
}

function parseArgs(str) {
  if (!str) return [];
  if (Array.isArray(str)) return str;
  const out = [];
  const re = /"([^"]*)"|(\S+)/g;
  let m;
  while ((m = re.exec(str))) out.push(m[1] !== undefined ? m[1] : m[2]);
  return out;
}

module.exports = { normPath, isInside, exists, isDir, hashId, fixPath, cleanName, matchKey, similarity, parseArgs };
