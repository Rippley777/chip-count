import { spawn } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const targets = new Set(['aarch64-apple-darwin', 'x86_64-apple-darwin', 'universal-apple-darwin']);

export function assertDeveloperIdSignature(details) {
  if (!/^Authority=Developer ID Application:/m.test(details)) {
    throw new Error(
      'The app must be signed with a Developer ID Application certificate. Run npm run macos:build with APPLE_SIGNING_IDENTITY set; an ad-hoc signature is not sufficient.',
    );
  }
  if (
    !/^TeamIdentifier=[A-Z0-9]+$/m.test(details) ||
    !/flags=.*\(.*runtime.*\)/.test(details) ||
    !/^Timestamp=.+$/m.test(details)
  ) {
    throw new Error(
      'The signature must include a TeamIdentifier, hardened runtime, and secure timestamp. Rebuild with npm run macos:build.',
    );
  }
}

export function assertAcceptedNotarization(result) {
  if (result.status !== 'Accepted') {
    throw new Error(
      `Apple returned ${result.status || 'an unknown status'} (submission ${result.id || 'unknown'}). Run npm run macos:notarization:log to inspect the saved submission. Stapling and release packaging stopped.`,
    );
  }
}

// Arguments are passed directly to subprocesses, never evaluated by a shell.
async function runCommand(command, args, options = {}) {
  console.log(`> ${command} ${args.join(' ')}`);
  return new Promise((resolvePromise, reject) => {
    const child = spawn(command, args, {
      cwd: root,
      env: options.env || process.env,
      stdio: options.interactive ? 'inherit' : ['ignore', 'pipe', 'pipe'],
    });
    let stdout = '';
    let stderr = '';
    child.stdout?.on('data', (chunk) => {
      stdout += chunk;
    });
    child.stderr?.on('data', (chunk) => {
      stderr += chunk;
    });
    child.on('error', reject);
    child.on('close', (code) => {
      if (code !== 0) reject(new Error(`${command} failed (${code}).\n${stderr || stdout}`));
      else resolvePromise({ stdout, stderr });
    });
  });
}

export function createWorkflow({
  target,
  env = process.env,
  platform = process.platform,
  run = runCommand,
  exists = existsSync,
  read = (path) => readFileSync(path, 'utf8'),
  write = (path, value) => writeFileSync(path, value),
  log = console.log,
} = {}) {
  if (platform !== 'darwin') throw new Error('Apple signing and notarization must run on macOS.');
  if (target && !targets.has(target)) throw new Error(`Unsupported macOS target: ${target}`);
  const config = JSON.parse(readFileSync(resolve(root, 'src-tauri/tauri.conf.json'), 'utf8'));
  const targetDir = resolve(root, env.CARGO_TARGET_DIR || 'target');
  const selectedTarget = target || env.CARGO_BUILD_TARGET;
  if (selectedTarget && !targets.has(selectedTarget))
    throw new Error(`Unsupported macOS target: ${selectedTarget}`);
  const bundleDir = resolve(
    targetDir,
    ...(selectedTarget ? [selectedTarget] : []),
    'release/bundle/macos',
  );
  const app = resolve(bundleDir, `${config.productName}.app`);
  const archive = resolve(bundleDir, `${config.productName}-macOS.zip`);
  const resultPath = resolve(bundleDir, 'notarization-result.json');
  const logPath = resolve(bundleDir, 'notarization-log.json');
  const profile = env.MACOS_NOTARY_PROFILE || 'chip-count-notary';
  const auth = ['--keychain-profile', profile];

  async function signed() {
    if (!exists(app))
      throw new Error(
        `App not found: ${app}. Run npm run macos:build first (use the same --target for every step).`,
      );
    try {
      await run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', app]);
    } catch (error) {
      throw new Error(
        `${error.message}\nRebuild a complete signed bundle with npm run macos:build before packaging or submission.`,
      );
    }
    const details = await run('codesign', ['--display', '--verbose=4', app]);
    assertDeveloperIdSignature(details.stderr + details.stdout);
  }

  async function build() {
    const identity = env.APPLE_SIGNING_IDENTITY;
    if (!identity?.startsWith('Developer ID Application:')) {
      throw new Error(
        'Set APPLE_SIGNING_IDENTITY to the full Developer ID Application identity shown by npm run macos:doctor.',
      );
    }
    const identities = await run('security', ['find-identity', '-v', '-p', 'codesigning']);
    if (!identities.stdout.includes(`"${identity}"`)) {
      throw new Error(
        'The requested Developer ID Application identity is not available. Install the certificate and its private key in your login Keychain.',
      );
    }
    const buildEnv = { ...env };
    // Notarization is handled explicitly below with the Keychain profile, not by Tauri.
    for (const key of [
      'APPLE_ID',
      'APPLE_PASSWORD',
      'APPLE_TEAM_ID',
      'APPLE_API_KEY',
      'APPLE_API_ISSUER',
      'APPLE_API_KEY_PATH',
    ])
      delete buildEnv[key];
    const args = [
      resolve(root, 'node_modules/@tauri-apps/cli/tauri.js'),
      'build',
      '--bundles',
      'app',
      '--config',
      resolve(root, 'src-tauri/tauri.macos-release.conf.json'),
    ];
    if (selectedTarget) args.push('--target', selectedTarget);
    await run(process.execPath, args, { env: buildEnv, interactive: true });
    await signed();
  }

  async function pack() {
    await signed();
    await run('ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', app, archive]);
    log(`ZIP: ${archive}`);
  }

  async function notarize() {
    await pack();
    const submitted = await run('xcrun', [
      'notarytool',
      'submit',
      archive,
      ...auth,
      '--wait',
      '--output-format',
      'json',
    ]);
    const result = JSON.parse(submitted.stdout);
    write(resultPath, JSON.stringify(result, null, 2) + '\n');
    assertAcceptedNotarization(result);
    log(`Apple accepted submission ${result.id}. Result: ${resultPath}`);
  }

  async function verify() {
    await signed();
    await run('xcrun', ['stapler', 'validate', app]);
    const assessment = await run('spctl', ['--assess', '--type', 'execute', '--verbose=4', app]);
    const details = assessment.stderr + assessment.stdout;
    if (!/\baccepted\b/.test(details) || !/source=Notarized Developer ID/.test(details)) {
      throw new Error(
        'Gatekeeper did not report acceptance from Notarized Developer ID. Inspect the spctl assessment before distributing.',
      );
    }
    log(details.trim());
    log('Developer ID signature, stapled ticket, and Gatekeeper assessment passed.');
  }

  async function staple() {
    await signed();
    await run('xcrun', ['stapler', 'staple', app]);
    await verify();
    // ZIP archives cannot carry a stapled ticket: staple the app, then recreate its ZIP.
    await pack();
  }

  async function notarizationLog(submissionId) {
    const id = submissionId || JSON.parse(read(resultPath)).id;
    if (!/^[a-f\d]{8}-[a-f\d]{4}-[a-f\d]{4}-[a-f\d]{4}-[a-f\d]{12}$/i.test(id || '')) {
      throw new Error(
        'Provide a valid Apple submission UUID: npm run macos:notarization:log -- SUBMISSION_ID',
      );
    }
    await run('xcrun', ['notarytool', 'log', id, ...auth, logPath]);
    log(`Apple notarization log: ${logPath}`);
  }

  return {
    paths: { app, archive, resultPath },
    async doctor() {
      for (const [command, args] of [
        ['xcode-select', ['-p']],
        ['xcrun', ['--find', 'notarytool']],
        ['xcrun', ['--find', 'stapler']],
        ['security', ['find-identity', '-v', '-p', 'codesigning']],
      ]) {
        const result = await run(command, args);
        log((result.stdout + result.stderr).trim());
      }
      log(`Notary Keychain profile: ${profile}\nApp: ${app}\nZIP: ${archive}`);
    },
    async credentials() {
      await run('xcrun', ['notarytool', 'store-credentials', profile], { interactive: true });
    },
    build,
    package: pack,
    notarize,
    staple,
    verify,
    log: notarizationLog,
    async release() {
      await build();
      await notarize();
      await staple();
      log(`Release ready: ${archive}`);
    },
  };
}

async function main() {
  const [action = 'help', ...args] = process.argv.slice(2);
  if (action === 'help') {
    console.log(
      'macOS release: doctor | credentials | build | package | notarize | staple | verify | log [SUBMISSION_ID] | release\nOptional: --target aarch64-apple-darwin | x86_64-apple-darwin | universal-apple-darwin\nSetup: docs/NOTARIZATION.md',
    );
    return;
  }
  const targetIndex = args.indexOf('--target');
  const target = targetIndex >= 0 ? args.splice(targetIndex, 2)[1] : undefined;
  if (targetIndex >= 0 && !target) throw new Error('--target requires a macOS target triple.');
  if (args.length > (action === 'log' ? 1 : 0))
    throw new Error('Unexpected arguments; run node scripts/macos-release.mjs help.');
  const workflow = createWorkflow({ target });
  if (!Object.hasOwn(workflow, action) || typeof workflow[action] !== 'function')
    throw new Error(`Unknown action: ${action}`);
  await workflow[action](...args);
}

if (
  process.argv[1] &&
  import.meta.url ===
    pathToFileURL(isAbsolute(process.argv[1]) ? process.argv[1] : resolve(process.argv[1])).href
) {
  main().catch((error) => {
    console.error(`macOS release failed: ${error.message}`);
    process.exitCode = 1;
  });
}
