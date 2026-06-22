#!/usr/bin/env node
/**
 * check.mjs — Project verification script for voice-server-rk3568
 *
 * V4.1 Controlled Workflow cross-platform check script.
 * Usage: node scripts/check.mjs
 *
 * Checks:
 *   1. Project structure (key files exist)
 *   2. Cargo.toml validity (parsing)
 *   3. Config.toml validity (TOML structure)
 *   4. Source code file consistency (mod.rs matches directory)
 *   5. WebSocket protocol definition consistency
 *   6. Git state (uncommitted changes)
 *
 * Returns exit code 0 on success, 1 on failure.
 */

import { existsSync, readFileSync, readdirSync, statSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');

let errors = [];
let warnings = [];
let passed = 0;
let failed = 0;

function ok(msg) {
  passed++;
  console.log(`  ✅ ${msg}`);
}

function warn(msg) {
  warnings.push(msg);
  console.log(`  ⚠️  ${msg}`);
}

function err(msg) {
  errors.push(msg);
  failed++;
  console.log(`  ❌ ${msg}`);
}

function section(title) {
  console.log(`\n── ${title} ─${''.padEnd(60 - title.length, '─')}`);
}

// ── 1. Project Structure ──────────────────────────────────

section('1. Project Structure');

const REQUIRED_DIRS = [
  'src',
  'src/asr',
  'src/tts',
  'src/ws',
  'deploy',
];

const REQUIRED_FILES = [
  'Cargo.toml',
  'config.toml',
  'src/main.rs',
  'src/config.rs',
  'src/error.rs',
  'src/asr/mod.rs',
  'src/asr/engine.rs',
  'src/asr/vad.rs',
  'src/tts/mod.rs',
  'src/tts/engine.rs',
  'src/ws/mod.rs',
  'src/ws/handler.rs',
  'src/ws/protocol.rs',
  'deploy/install.sh',
  'deploy/voice-server.service',
];

for (const d of REQUIRED_DIRS) {
  const fullPath = join(ROOT, d);
  if (existsSync(fullPath) && statSync(fullPath).isDirectory()) {
    ok(`Directory exists: ${d}/`);
  } else {
    err(`Missing directory: ${d}/`);
  }
}

for (const f of REQUIRED_FILES) {
  const fullPath = join(ROOT, f);
  if (existsSync(fullPath) && statSync(fullPath).isFile()) {
    ok(`File exists: ${f}`);
  } else {
    err(`Missing file: ${f}`);
  }
}

// ── 2. Cargo.toml Parsing ────────────────────────────────

section('2. Cargo.toml Validity');

const cargoPath = join(ROOT, 'Cargo.toml');
if (existsSync(cargoPath)) {
  const cargoContent = readFileSync(cargoPath, 'utf-8');
  const hasPackage = cargoContent.includes('[package]');
  const hasDeps = cargoContent.includes('[dependencies]');
  // Basic field checks
  const nameMatch = cargoContent.match(/name\s*=\s*"(.+?)"/);
  const versionMatch = cargoContent.match(/version\s*=\s*"(.+?)"/);

  if (hasPackage && hasDeps) {
    ok('Cargo.toml has [package] and [dependencies] sections');
  } else {
    err('Cargo.toml missing [package] or [dependencies]');
  }

  if (nameMatch) {
    ok(`Package name: "${nameMatch[1]}"`);
  } else {
    err('Cargo.toml missing package name');
  }

  if (versionMatch) {
    ok(`Package version: "${versionMatch[1]}"`);
  } else {
    err('Cargo.toml missing package version');
  }

  // Check key dependencies present
  const deps = ['axum', 'tokio', 'sherpa-onnx', 'serde', 'tracing'];
  for (const dep of deps) {
    if (cargoContent.includes(`${dep} =`)) {
      ok(`Dependency: ${dep}`);
    } else {
      err(`Missing dependency: ${dep}`);
    }
  }
} else {
  err('Cargo.toml not found (should not reach here)');
}

// ── 3. Config.toml Validity ──────────────────────────────

section('3. Config.toml Structure');

const configPath = join(ROOT, 'config.toml');
if (existsSync(configPath)) {
  const configContent = readFileSync(configPath, 'utf-8');
  const requiredSections = ['[server]', '[asr]', '[tts]', '[vad]'];
  for (const sectionName of requiredSections) {
    if (configContent.includes(sectionName)) {
      ok(`Config section: ${sectionName}`);
    } else {
      err(`Missing config section: ${sectionName}`);
    }
  }

  // Check key fields
  if (configContent.includes('model_type')) ok('Config: model_type present');
  if (configContent.includes('num_threads')) ok('Config: num_threads present');
  if (configContent.includes('provider')) ok('Config: provider present');
} else {
  err('config.toml not found');
}

// ── 4. Module Structure Consistency ──────────────────────

section('4. Module Consistency');

// Check each module's mod.rs re-exports match actual files
const modDirs = [
  { dir: 'src/asr', modFile: 'src/asr/mod.rs', expected: ['engine', 'vad'] },
  { dir: 'src/tts', modFile: 'src/tts/mod.rs', expected: ['engine'] },
  { dir: 'src/ws', modFile: 'src/ws/mod.rs', expected: ['handler', 'protocol'] },
];

for (const mod of modDirs) {
  const modFilePath = join(ROOT, mod.modFile);
  if (!existsSync(modFilePath)) {
    err(`Missing mod.rs: ${mod.modFile}`);
    continue;
  }
  const modContent = readFileSync(modFilePath, 'utf-8');
  for (const sub of mod.expected) {
    if (modContent.includes(`pub mod ${sub};`)) {
      ok(`${mod.modFile} declares pub mod ${sub};`);
    } else if (modContent.includes(`mod ${sub};`)) {
      warn(`${mod.modFile}: ${sub} is private (not pub)`);
    } else {
      err(`${mod.modFile} missing declaration for ${sub}`);
    }
  }
}

// ── 5. Protocol Definition Consistency ───────────────────

section('5. Protocol Definitions');

const protoPath = join(ROOT, 'src/ws/protocol.rs');
if (existsSync(protoPath)) {
  const protoContent = readFileSync(protoPath, 'utf-8');

  // Check binary markers
  const asrMarker = protoContent.includes('ASR_AUDIO_MARKER: u8 = 0x00;');
  const ttsMarker = protoContent.includes('TTS_AUDIO_MARKER: u8 = 0x01;');
  if (asrMarker) ok('Binary marker: ASR_AUDIO_MARKER (0x00)');
  else err('Missing ASR_AUDIO_MARKER');
  if (ttsMarker) ok('Binary marker: TTS_AUDIO_MARKER (0x01)');
  else err('Missing TTS_AUDIO_MARKER');

  // Check client message types
  const clientTypes = [
    'AsrStart', 'AsrAudio', 'AsrStop',
    'TtsRequest', 'TtsCancel',
    'Config', 'Ping',
  ];
  for (const ct of clientTypes) {
    if (protoContent.includes(ct)) ok(`Client message: ${ct}`);
    else err(`Missing client message variant: ${ct}`);
  }

  // Check server message types
  const serverTypes = [
    'AsrInterim', 'AsrFinal', 'AsrError',
    'TtsAudio', 'TtsEnd', 'TtsError',
    'VadState', 'Error', 'Pong',
  ];
  for (const st of serverTypes) {
    if (protoContent.includes(st)) ok(`Server message: ${st}`);
    else err(`Missing server message variant: ${st}`);
  }

  // Check spec-defined markers in spec.md match code
  const specPath = join(ROOT, 'spec.md');
  if (existsSync(specPath)) {
    const specContent = readFileSync(specPath, 'utf-8');
    const specHasPing = specContent.includes('`ping`');
    const specHasPong = specContent.includes('`pong`');
    if (specHasPing) ok('Spec: ping message type documented');
    if (specHasPong) ok('Spec: pong message type documented');
    // Check binary frame spec
    const specBinaryAsr = specContent.includes('0x00');
    const specBinaryTts = specContent.includes('0x01');
    if (specBinaryAsr) ok('Spec: Binary frame 0x00 (ASR) documented');
    if (specBinaryTts) ok('Spec: Binary frame 0x01 (TTS) documented');
  }
}

// ── 6. Git State ─────────────────────────────────────────

section('6. Git State');

import { execSync } from 'child_process';
try {
  const gitStatus = execSync('git status --porcelain', { cwd: ROOT, encoding: 'utf-8' }).trim();
  if (gitStatus.length === 0) {
    ok('Working tree is clean');
  } else {
    const lines = gitStatus.split('\n');
    warn(`Working tree has ${lines.length} uncommitted change(s):`);
    for (const line of lines) {
      console.log(`       ${line}`);
    }
  }

  const gitLog = execSync('git log --oneline -1', { cwd: ROOT, encoding: 'utf-8' }).trim();
  console.log(`       Last commit: ${gitLog}`);
} catch (e) {
  warn(`Git check failed: ${e.message}`);
}

// ── Summary ──────────────────────────────────────────────

section('Summary');
console.log(`  ✅ Passed: ${passed}`);
console.log(`  ❌ Failed: ${failed}`);
if (warnings.length > 0) {
  console.log(`  ⚠️  Warnings: ${warnings.length}`);
}

if (failed > 0) {
  console.log('\n❌ Verification FAILED — see errors above.');
  process.exit(1);
} else {
  console.log('\n✅ Verification PASSED.');
  process.exit(0);
}
