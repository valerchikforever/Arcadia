'use strict';
// Minimal parser for Valve KeyValues text files (.vdf / .acf)

function parseVDF(text) {
  const root = {};
  const stack = [root];
  let key = null;
  let i = 0;
  const n = text.length;
  const isWs = (c) => c === ' ' || c === '\t' || c === '\r' || c === '\n' || c === '﻿';

  while (i < n) {
    const c = text[i];
    if (c === '/' && text[i + 1] === '/') {
      while (i < n && text[i] !== '\n') i++;
      continue;
    }
    if (isWs(c)) { i++; continue; }
    if (c === '{') {
      const obj = {};
      if (key !== null) stack[stack.length - 1][key] = obj;
      stack.push(obj);
      key = null;
      i++;
      continue;
    }
    if (c === '}') {
      if (stack.length > 1) stack.pop();
      key = null;
      i++;
      continue;
    }
    let tok = '';
    if (c === '"') {
      i++;
      while (i < n && text[i] !== '"') {
        if (text[i] === '\\' && i + 1 < n) {
          const nx = text[i + 1];
          tok += nx === 'n' ? '\n' : nx === 't' ? '\t' : nx;
          i += 2;
          continue;
        }
        tok += text[i++];
      }
      i++;
    } else {
      while (i < n && !isWs(text[i]) && text[i] !== '{' && text[i] !== '}' && text[i] !== '"') tok += text[i++];
      // skip conditional blocks like [$WIN32]
      if (tok.startsWith('[') && tok.endsWith(']')) continue;
    }
    if (key === null) key = tok;
    else { stack[stack.length - 1][key] = tok; key = null; }
  }
  return root;
}

// Case-insensitive property lookup
function ci(obj, name) {
  if (!obj || typeof obj !== 'object') return undefined;
  if (name in obj) return obj[name];
  const low = name.toLowerCase();
  for (const k of Object.keys(obj)) if (k.toLowerCase() === low) return obj[k];
  return undefined;
}

function ciPath(obj, ...names) {
  let cur = obj;
  for (const nm of names) { cur = ci(cur, nm); if (cur === undefined) return undefined; }
  return cur;
}

module.exports = { parseVDF, ci, ciPath };
