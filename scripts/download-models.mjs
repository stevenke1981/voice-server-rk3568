#!/usr/bin/env node
/**
 * download-models.mjs — Model download helper for voice-server-rk3568
 *
 * Downloads recommended Sherpa-ONNX models for ASR, TTS, and VAD.
 * Usage:
 *   node scripts/download-models.mjs --list         # list available models
 *   node scripts/download-models.mjs --all           # download all (default)
 *   node scripts/download-models.mjs --asr --tts     # selective
 *   node scripts/download-models.mjs --all --dir /opt/voice-server/models
 *
 * Models are downloaded to ./models/ by default.
 * Set MODEL_DIR env var or use --dir to change destination.
 */

import { existsSync, mkdirSync, writeFileSync, readFileSync, createWriteStream, unlinkSync, renameSync } from 'fs';
import { join, dirname, basename } from 'path';
import { fileURLToPath } from 'url';
import { get } from 'https';
import { execSync } from 'child_process';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');

function getModelDir() {
  return process.env.MODEL_DIR || join(ROOT, 'models');
}

const ASR_BASE = 'https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models';
const TTS_BASE = 'https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models';

const MODELS = {
  asr: {
    'zipformer-en-20m': {
      description: 'Zipformer EN streaming 20M (English, ~40MB extracted)',
      archive: `${ASR_BASE}/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17.tar.bz2`,
      extractDir: 'sherpa-onnx-streaming-zipformer-en-20M-2023-02-17',
      files: ['encoder-epoch-99-avg-1.int8.onnx', 'decoder-epoch-99-avg-1.int8.onnx', 'joiner-epoch-99-avg-1.int8.onnx', 'tokens.txt'],
      configType: 'transducer',
      fileMap: {
        'encoder-epoch-99-avg-1.int8.onnx': 'encoder.onnx',
        'decoder-epoch-99-avg-1.int8.onnx': 'decoder.onnx',
        'joiner-epoch-99-avg-1.int8.onnx': 'joiner.onnx',
      },
    },
    'zipformer-zh-14m': {
      description: 'Zipformer ZH streaming 14M (Chinese, ~25MB extracted)',
      archive: `${ASR_BASE}/sherpa-onnx-streaming-zipformer-zh-14M-2023-02-23.tar.bz2`,
      extractDir: 'sherpa-onnx-streaming-zipformer-zh-14M-2023-02-23',
      files: ['encoder-epoch-99-avg-1.int8.onnx', 'decoder-epoch-99-avg-1.int8.onnx', 'joiner-epoch-99-avg-1.int8.onnx', 'tokens.txt'],
      configType: 'transducer',
      fileMap: {
        'encoder-epoch-99-avg-1.int8.onnx': 'encoder.onnx',
        'decoder-epoch-99-avg-1.int8.onnx': 'decoder.onnx',
        'joiner-epoch-99-avg-1.int8.onnx': 'joiner.onnx',
      },
    },
  },
  tts: {
    'vits-melo-zh-en': {
      description: 'VITS Melo TTS (Chinese + English, ~163MB, 1 speaker)',
      archive: `${TTS_BASE}/vits-melo-tts-zh_en.tar.bz2`,
      extractDir: 'vits-melo-tts-zh_en',
      files: ['model.onnx', 'tokens.txt', 'lexicon.txt'],
      configType: 'vits',
    },
    'vits-ljs-en': {
      description: 'VITS LJSpeech (English, ~109MB, 1 speaker)',
      archive: `${TTS_BASE}/vits-ljs.tar.bz2`,
      extractDir: 'vits-ljs',
      files: ['vits-ljs.onnx', 'tokens.txt', 'lexicon.txt'],
      configType: 'vits',
    },
    'vits-glados-en': {
      description: 'VITS Piper GLaDOS (English, ~61MB, 1 speaker)',
      archive: `${TTS_BASE}/vits-piper-en_US-glados.tar.bz2`,
      extractDir: 'vits-piper-en_US-glados',
      files: ['en_US-glados.onnx', 'tokens.txt', 'espeak-ng-data'],
      configType: 'vits',
    },
  },
  vad: {
    'silero-vad': {
      description: 'Silero VAD v5 INT8 (Voice Activity Detection, ~208KB)',
      archive: `${ASR_BASE}/silero_vad.int8.onnx`,
      isSingleFile: true,
      targetName: 'silero_vad.onnx',
      configType: 'silero-vad',
    },
  },
};

// ── Helpers ────────────────────────────────────────────

function downloadFile(url, dest) {
  return new Promise((resolve, reject) => {
    const file = createWriteStream(dest);
    get(url, (response) => {
      if (response.statusCode >= 300 && response.statusCode < 400 && response.headers.location) {
        file.close();
        unlinkSync(dest);
        downloadFile(response.headers.location, dest).then(resolve).catch(reject);
        return;
      }
      if (response.statusCode !== 200) {
        file.close();
        if (existsSync(dest)) unlinkSync(dest);
        reject(new Error(`HTTP ${response.statusCode}: ${url}`));
        return;
      }
      const total = parseInt(response.headers['content-length'], 10);
      let downloaded = 0;
      response.on('data', (chunk) => {
        downloaded += chunk.length;
        if (total) {
          const pct = ((downloaded / total) * 100).toFixed(1);
          process.stdout.write(`\r  ${pct}% (${(downloaded / 1024 / 1024).toFixed(1)}MB / ${(total / 1024 / 1024).toFixed(1)}MB)`);
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

  // Accept --dir <path> to set download directory
  const dirIdx = args.indexOf('--dir');
  if (dirIdx !== -1 && dirIdx + 1 < args.length) {
    process.env.MODEL_DIR = args[dirIdx + 1];
  }

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

  // Pick first model from each selected category
  const selected = {};
  if (selections.includes('asr')) {
    const keys = Object.keys(MODELS.asr);
    selected.asr = keys[0];
  }
  if (selections.includes('tts')) {
    const keys = Object.keys(MODELS.tts);
    selected.tts = keys[0];
  }
  if (selections.includes('vad')) {
    const keys = Object.keys(MODELS.vad);
    selected.vad = keys[0];
  }

  const modelDir = getModelDir();
  console.log(`\nDownload directory: ${modelDir}`);
  console.log('Models to download:');
  for (const [cat, key] of Object.entries(selected)) {
    console.log(`  ${cat}: ${key} — ${MODELS[cat][key].description}`);
  }
  console.log();

  const tmpDir = join(modelDir, '.download_tmp');
  if (!existsSync(tmpDir)) mkdirSync(tmpDir, { recursive: true });

  for (const [cat, key] of Object.entries(selected)) {
    const model = MODELS[cat][key];
    const destDir = join(modelDir, cat);
    if (!existsSync(destDir)) mkdirSync(destDir, { recursive: true });

    if (model.isSingleFile) {
      // Single file download (e.g., VAD)
      const dest = join(destDir, model.targetName);
      if (existsSync(dest)) {
        console.log(`  ⏭  ${model.targetName} already exists (${(readFileSync(dest).length / 1024).toFixed(0)}KB)`);
        continue;
      }
      console.log(`📥 Downloading ${cat}/${key}...`);
      try {
        await downloadFile(model.archive, dest);
        const size = (readFileSync(dest).length / 1024 / 1024).toFixed(1);
        console.log(`  ✅ ${model.targetName} (${size}MB)`);
      } catch (e) {
        console.error(`  ❌ Failed: ${e.message}`);
      }
    } else {
      // tar.bz2 archive download + extract
      const archiveName = basename(model.archive);
      const archivePath = join(tmpDir, archiveName);
      const extractPath = join(tmpDir, model.extractDir);

      if (existsSync(extractPath) && model.files.every((f) => existsSync(join(extractPath, f)))) {
        console.log(`  ⏭  ${key} already extracted`);
      } else {
        // Download archive
        if (!existsSync(archivePath)) {
          console.log(`📥 Downloading ${cat}/${key}...`);
          console.log(`  Archive: ${archiveName}`);
          try {
            await downloadFile(model.archive, archivePath);
          } catch (e) {
            console.error(`  ❌ Download failed: ${e.message}`);
            continue;
          }
        } else {
          console.log(`  ⏭  ${archiveName} already downloaded`);
        }

        // Extract tar.bz2
        console.log(`📦 Extracting ${archiveName}...`);
        try {
          execSync(`tar xf "${archivePath}" -C "${tmpDir}"`, { stdio: 'pipe' });
          console.log(`  ✅ Extracted to ${model.extractDir}/`);
        } catch (e) {
          console.error(`  ❌ Extract failed: ${e.message}`);
          continue;
        }

        // Clean up archive
        unlinkSync(archivePath);
      }

      // Copy extracted files to destination
      if (existsSync(extractPath)) {
        for (const file of model.files) {
          const src = join(extractPath, file);
          const dst = join(destDir, file);
          if (existsSync(src)) {
            if (existsSync(dst)) {
              console.log(`  ⏭  ${file} already in destination`);
            } else {
              // Use cp via exec for directories (e.g., espeak-ng-data)
              execSync(`cp -r "${src}" "${dst}"`, { stdio: 'pipe' });
              const size = existsSync(dst) ? (readFileSync(dst).length / 1024 / 1024).toFixed(1) + 'MB' : '(dir)';
              console.log(`  ✅ ${file} (${size})`);
            }
          } else {
            console.log(`  ⚠ ${file} not found in extracted archive`);
          }
        }
        // Create config-friendly symlinks if fileMap is defined
        if (model.fileMap) {
          for (const [srcName, linkName] of Object.entries(model.fileMap)) {
            const srcPath = join(destDir, srcName);
            const linkPath = join(destDir, linkName);
            if (existsSync(srcPath) && !existsSync(linkPath)) {
              execSync(`ln -sf "${srcName}" "${linkPath}"`, { stdio: 'pipe' });
              console.log(`  🔗 ${linkName} -> ${srcName}`);
            }
          }
        }
        // Clean up extracted directory
        execSync(`rm -rf "${extractPath}"`, { stdio: 'pipe' });
      }
    }
  }

  // Clean up temp
  if (existsSync(tmpDir)) {
    execSync(`rm -rf "${tmpDir}"`, { stdio: 'pipe' });
  }

  console.log('\n✅ Download complete.');

  // Write manifest
  const manifest = {
    downloaded_at: new Date().toISOString(),
    target_device: 'RK3568',
    models: {},
  };
  for (const [cat, key] of Object.entries(selected)) {
    const model = MODELS[cat][key];
    const destDir = join(modelDir, cat);
    if (model.isSingleFile) {
      const path = join(destDir, model.targetName);
      if (existsSync(path)) {
        manifest.models[`${cat}/${key}`] = {
          files: { [model.targetName]: readFileSync(path).length },
        };
      }
    } else {
      const files = {};
      for (const f of model.files) {
        const path = join(destDir, f);
        if (existsSync(path)) {
          files[f] = readFileSync(path).length;
        }
      }
      manifest.models[`${cat}/${key}`] = { files };
    }
  }
  const manifestPath = join(modelDir, 'manifest.json');
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));
  console.log(`  📋 Manifest: ${manifestPath}`);
}

main().catch((e) => {
  console.error(`Fatal error: ${e.message}`);
  process.exit(1);
});
