#!/usr/bin/env node
/**
 * download-models.mjs — Model download helper for voice-server-rk3568
 *
 * Downloads recommended Sherpa-ONNX models for ASR, TTS, and VAD.
 * Usage:
 *   node scripts/download-models.mjs              # interactive prompt
 *   node scripts/download-models.mjs --all         # download all
 *   node scripts/download-models.mjs --asr --tts   # selective
 *   node scripts/download-models.mjs --list        # list available models
 *
 * Models are downloaded to ./models/ by default.
 * Set MODEL_DIR env var to change destination.
 */

const BASE_URL = 'https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models';

const MODELS = {
  asr: {
    'zipformer-en-20m': {
      description: 'Zipformer EN streaming 20M (English, ~20MB)',
      files: {
        'encoder.onnx': `${BASE_URL}/sherpa-onnx-streaming-zipformer-20M-2023-02-17/encoder-epoch-99-avg-1.onnx`,
        'decoder.onnx': `${BASE_URL}/sherpa-onnx-streaming-zipformer-20M-2023-02-17/decoder-epoch-99-avg-1.onnx`,
        'joiner.onnx': `${BASE_URL}/sherpa-onnx-streaming-zipformer-20M-2023-02-17/joiner-epoch-99-avg-1.onnx`,
        'tokens.txt': `${BASE_URL}/sherpa-onnx-streaming-zipformer-20M-2023-02-17/tokens.txt`,
      },
    },
    'zipformer-zh-20m': {
      description: 'Zipformer ZH streaming 20M (Chinese, ~20MB)',
      files: {
        'encoder.onnx': `${BASE_URL}/sherpa-onnx-streaming-zipformer-zh-20M-2023-02-17/encoder-epoch-99-avg-1.onnx`,
        'decoder.onnx': `${BASE_URL}/sherpa-onnx-streaming-zipformer-zh-20M-2023-02-17/decoder-epoch-99-avg-1.onnx`,
        'joiner.onnx': `${BASE_URL}/sherpa-onnx-streaming-zipformer-zh-20M-2023-02-17/joiner-epoch-99-avg-1.onnx`,
        'tokens.txt': `${BASE_URL}/sherpa-onnx-streaming-zipformer-zh-20M-2023-02-17/tokens.txt`,
      },
    },
    'sense-voice-small': {
      description: 'SenseVoice Small (Multi-language, ~240MB)',
      files: {
        'model.onnx': `${BASE_URL}/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/model.int8.onnx`,
        'tokens.txt': `${BASE_URL}/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/tokens.txt`,
      },
    },
  },
  tts: {
    'vits-zh': {
      description: 'VITS Chinese (Neural TTS, ~50MB)',
      files: {
        'model.onnx': `https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/vits-zh-aishell3-model.onnx`,
        'tokens.txt': `https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/vits-zh-aishell3-tokens.txt`,
      },
    },
    'vits-en': {
      description: 'VITS English (Neural TTS, ~50MB)',
      files: {
        'model.onnx': `https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/vits-vctk-model.onnx`,
        'tokens.txt': `https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/vits-vctk-tokens.txt`,
      },
    },
  },
  vad: {
    'silero-vad': {
      description: 'Silero VAD v5 INT8 (Voice Activity Detection, ~5MB)',
      files: {
        'silero_vad.onnx': `${BASE_URL}/silero_vad_5_2023-08-23/silero_vad.onnx`,
      },
    },
  },
};

// ── Helpers ────────────────────────────────────────────

import { existsSync, mkdirSync, writeFileSync, readFileSync, createWriteStream, unlinkSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { get } from 'https';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');
const MODEL_DIR = process.env.MODEL_DIR || join(ROOT, 'models');

function download(url, dest) {
  return new Promise((resolve, reject) => {
    const file = createWriteStream(dest);
    get(url, (response) => {
      if (response.statusCode >= 300 && response.statusCode < 400 && response.headers.location) {
        file.close();
        download(response.headers.location, dest).then(resolve).catch(reject);
        return;
      }
      if (response.statusCode !== 200) {
        file.close();
        unlinkSync(dest);
        reject(new Error(`HTTP ${response.statusCode}: ${url}`));
        return;
      }
      const total = parseInt(response.headers['content-length'], 10);
      let downloaded = 0;
      response.on('data', (chunk) => {
        downloaded += chunk.length;
        if (total) {
          const pct = ((downloaded / total) * 100).toFixed(1);
          process.stdout.write(`\r  Downloading: ${pct}% (${(downloaded / 1024 / 1024).toFixed(1)}MB / ${(total / 1024 / 1024).toFixed(1)}MB)`);
        }
      });
      response.pipe(file);
      file.on('finish', () => {
        file.close();
        process.stdout.write('\n');
        resolve();
      });
    }).on('error', (err) => {
      file.close();
      if (existsSync(dest)) unlinkSync(dest);
      reject(err);
    });
  });
}

// ── Main ───────────────────────────────────────────────

async function main() {
  const args = process.argv.slice(2);
  const isList = args.includes('--list');
  const downloadAll = args.includes('--all') || args.length === 0;

  if (isList) {
    console.log('\nAvailable models:\n');
    for (const [category, models] of Object.entries(MODELS)) {
      console.log(`  ${category.toUpperCase()}:`);
      for (const [key, model] of Object.entries(models)) {
        console.log(`    ${key.padEnd(20)} ${model.description}`);
      }
    }
    console.log();
    return;
  }

  const wantAsr = downloadAll || args.includes('--asr');
  const wantTts = downloadAll || args.includes('--tts');
  const wantVad = downloadAll || args.includes('--vad');

  const selections = [];
  if (wantAsr) selections.push('asr');
  if (wantTts) selections.push('tts');
  if (wantVad) selections.push('vad');

  if (selections.length === 0) {
    console.log('No category selected. Use --asr, --tts, --vad, or --all');
    process.exit(1);
  }

  // For simplicity, download the first model in each category
  const toDownload = {};
  if (selections.includes('asr')) toDownload.asr = 'zipformer-en-20m';
  if (selections.includes('tts')) toDownload.tts = 'vits-en';
  if (selections.includes('vad')) toDownload.vad = 'silero-vad';

  console.log(`\nDownload directory: ${MODEL_DIR}`);
  console.log('Models to download:');
  for (const [cat, key] of Object.entries(toDownload)) {
    console.log(`  ${cat}: ${key} — ${MODELS[cat][key].description}`);
  }
  console.log();

  for (const [cat, key] of Object.entries(toDownload)) {
    const model = MODELS[cat][key];
    const destDir = join(MODEL_DIR, cat);
    if (!existsSync(destDir)) mkdirSync(destDir, { recursive: true });

    console.log(`\n📥 Downloading ${cat}/${key}...`);
    for (const [filename, url] of Object.entries(model.files)) {
      const dest = join(destDir, filename);
      if (existsSync(dest)) {
        const size = (readFileSync(dest).length / 1024 / 1024).toFixed(1);
        console.log(`  Skipping ${filename} (already exists, ${size}MB)`);
        continue;
      }
      try {
        console.log(`  ${filename}`);
        console.log(`  From: ${url}`);
        await download(url, dest);
        const size = (readFileSync(dest).length / 1024 / 1024).toFixed(1);
        console.log(`  ✅ Saved: ${dest} (${size}MB)`);
      } catch (e) {
        console.error(`  ❌ Failed: ${e.message}`);
      }
    }
  }

  console.log('\n✅ Download complete.');

  // Write a model manifest
  const manifest = {
    downloaded_at: new Date().toISOString(),
    target_device: 'RK3568',
    models: {},
  };

  for (const [cat, key] of Object.entries(toDownload)) {
    const model = MODELS[cat][key];
    const destDir = join(MODEL_DIR, cat);
    manifest.models[cat] = {
      name: key,
      description: model.description,
      files: {},
    };
    for (const [filename] of Object.entries(model.files)) {
      const dest = join(destDir, filename);
      if (existsSync(dest)) {
        manifest.models[cat].files[filename] = {
          size_bytes: readFileSync(dest).length,
        };
      }
    }
  }

  const manifestPath = join(MODEL_DIR, 'manifest.json');
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));
  console.log(`  📋 Manifest: ${manifestPath}`);
}

main().catch((e) => {
  console.error(`Fatal error: ${e.message}`);
  process.exit(1);
});
