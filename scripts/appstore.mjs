import { spawn } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const identifier = JSON.parse(readFileSync(resolve(root, 'src-tauri/tauri.conf.json'))).identifier;

async function run(command, args, env = process.env) {
  return new Promise((accept, reject) => {
    const child = spawn(command, args, { cwd: root, env, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '',
      stderr = '';
    child.stdout.on('data', (chunk) => {
      stdout += chunk;
    });
    child.stderr.on('data', (chunk) => {
      stderr += chunk;
    });
    child.on('error', reject);
    child.on('close', (code) =>
      code === 0
        ? accept({ stdout, stderr })
        : reject(new Error(`${command} failed (${code}): ${stderr || stdout}`)),
    );
  });
}
export function validateProfile(profile, team, now = new Date()) {
  const ent = profile.Entitlements || {};
  if (!/^[A-Z0-9]{10}$/.test(team || '') || !profile.TeamIdentifier?.includes(team))
    throw new Error('APP_STORE_TEAM_ID must match the provisioning profile team.');
  if (
    ent['com.apple.application-identifier'] !== `${team}.${identifier}` ||
    ent['com.apple.developer.team-identifier'] !== team
  )
    throw new Error(`Provisioning profile must authorize ${team}.${identifier}.`);
  if (
    !profile.ExpirationDate ||
    !Number.isFinite(new Date(profile.ExpirationDate).getTime()) ||
    new Date(profile.ExpirationDate) <= now
  )
    throw new Error('The provisioning profile is expired or has no expiration date.');
  if (profile.ProvisionedDevices || profile.ProvisionsAllDevices || ent['get-task-allow'])
    throw new Error(
      'Use a Mac App Store Connect distribution profile, not a development or Developer ID profile.',
    );
}
export function validateSignature(text, local = false) {
  if (
    !local &&
    !/^Authority=(Apple Distribution:|3rd Party Mac Developer Application:)/m.test(text)
  )
    throw new Error('The app needs a Mac App Store distribution signature.');
  const expected = local ? `${identifier}.sandbox-test` : identifier;
  if (!text.split(/\r?\n/).includes(`Identifier=${expected}`))
    throw new Error('Unexpected bundle identifier.');
}
export async function workflow(
  mode,
  { env = process.env, platform = process.platform, execute = run } = {},
) {
  if (platform !== 'darwin') throw new Error('App Store builds require macOS.');
  if (!['local', 'build', 'package', 'verify'].includes(mode))
    throw new Error('Use local, build, package, or verify.');
  const local = mode === 'local';
  const output = resolve(root, 'artifacts', local ? 'sandbox-local' : 'appstore');
  mkdirSync(output, { recursive: true });
  const targetDir = resolve(output, 'target');
  const app = resolve(targetDir, local ? 'debug' : 'release', 'bundle/macos/Chip Count.app');
  const pkg = resolve(output, 'Chip Count.pkg');
  const configPath = resolve(output, 'tauri.generated.conf.json');
  const entitlementsPath = resolve(output, 'Entitlements.generated.plist');
  if (mode === 'build' || local) {
    let entitlements = readFileSync(resolve(root, 'src-tauri/Entitlements.appstore.plist'), 'utf8');
    const config = { bundle: { macOS: { signingIdentity: '-' } } };
    if (local) config.identifier = `${identifier}.sandbox-test`;
    else {
      const identity = env.APP_STORE_SIGNING_IDENTITY;
      if (!/^(Apple Distribution:|3rd Party Mac Developer Application:)/.test(identity || ''))
        throw new Error(
          'Set APP_STORE_SIGNING_IDENTITY to your Mac App Store application certificate identity.',
        );
      const profilePath = resolve(env.APP_STORE_PROFILE || '');
      if (!env.APP_STORE_PROFILE || !existsSync(profilePath))
        throw new Error('Set APP_STORE_PROFILE to a .provisionprofile file.');
      const identities = await execute(
        'security',
        ['find-identity', '-v', '-p', 'codesigning'],
        env,
      );
      if (!identities.stdout.includes(`"${identity}"`))
        throw new Error('The distribution certificate and private key are not installed.');
      const decoded = await execute('security', ['cms', '-D', '-i', profilePath], env);
      const decodedPath = resolve(output, 'profile.decoded.plist');
      writeFileSync(decodedPath, decoded.stdout, { mode: 0o600 });
      // Full profiles contain dates and certificate NSData, which cannot be converted
      // wholesale to JSON by plutil. Extract only the fields used for validation.
      const extract = async (key, format = 'json', optional = false) => {
        try {
          const result = await execute(
            'plutil',
            ['-extract', key, format, '-o', '-', decodedPath],
            env,
          );
          return format === 'json' ? JSON.parse(result.stdout) : result.stdout.trim();
        } catch (error) {
          if (optional) return undefined;
          throw error;
        }
      };
      const [TeamIdentifier, ExpirationDate, Entitlements, ProvisionedDevices, allDevices] =
        await Promise.all([
          extract('TeamIdentifier'),
          extract('ExpirationDate', 'raw'),
          extract('Entitlements'),
          extract('ProvisionedDevices', 'json', true),
          extract('ProvisionsAllDevices', 'raw', true),
        ]);
      validateProfile(
        {
          TeamIdentifier,
          ExpirationDate,
          Entitlements,
          ProvisionedDevices,
          ProvisionsAllDevices: allDevices === 'true',
        },
        env.APP_STORE_TEAM_ID,
      );
      entitlements = entitlements.replace(
        '</dict>',
        `<key>com.apple.application-identifier</key><string>${env.APP_STORE_TEAM_ID}.${identifier}</string>\n<key>com.apple.developer.team-identifier</key><string>${env.APP_STORE_TEAM_ID}</string>\n</dict>`,
      );
      config.bundle.macOS.signingIdentity = identity;
      config.bundle.macOS.files = { 'embedded.provisionprofile': profilePath };
    }
    writeFileSync(entitlementsPath, entitlements);
    config.bundle.macOS.entitlements = entitlementsPath;
    writeFileSync(configPath, JSON.stringify(config, null, 2));
    const buildEnv = {
      ...env,
      CARGO_TARGET_DIR: targetDir,
      VITE_APP_STORE: '1',
      ...(local ? { CARGO_PROFILE_DEV_DEBUG: '0', CARGO_INCREMENTAL: '0' } : {}),
    };
    // App Store distribution does not use the Developer ID notarization pipeline.
    for (const key of [
      'APPLE_ID',
      'APPLE_PASSWORD',
      'APPLE_TEAM_ID',
      'APPLE_API_KEY',
      'APPLE_API_ISSUER',
      'APPLE_API_KEY_PATH',
      'APPLE_SIGNING_IDENTITY',
    ])
      delete buildEnv[key];
    const args = [
      resolve(root, 'node_modules/@tauri-apps/cli/tauri.js'),
      'build',
      '--bundles',
      'app',
      '--config',
      resolve(root, 'src-tauri/tauri.appstore.conf.json'),
      '--config',
      configPath,
    ];
    if (local) args.push('--debug');
    args.push('--', '--no-default-features');
    console.log('Building the sandboxed App Store variant…');
    await execute(process.execPath, args, buildEnv);
  }
  await execute('codesign', ['--verify', '--deep', '--strict', app], env);
  const signature = await execute('codesign', ['--display', '--verbose=4', app], env);
  validateSignature(signature.stdout + signature.stderr, local);
  const signedEntitlements = await execute(
    'codesign',
    ['--display', '--entitlements', '-', '--xml', app],
    env,
  );
  const actualPath = resolve(output, 'signed-entitlements.plist');
  writeFileSync(actualPath, signedEntitlements.stdout);
  const actual = JSON.parse(
    (await execute('plutil', ['-convert', 'json', '-o', '-', actualPath], env)).stdout,
  );
  for (const key of [
    'com.apple.security.app-sandbox',
    'com.apple.security.files.user-selected.read-write',
    'com.apple.security.files.bookmarks.app-scope',
    'com.apple.security.network.client',
  ])
    if (actual[key] !== true) throw new Error(`The signed app is missing ${key}.`);
  for (const key of [
    'com.apple.security.temporary-exception.files.absolute-path.read-only',
    'com.apple.security.temporary-exception.files.home-relative-path.read-only',
    'com.apple.security.network.server',
  ])
    if (actual[key]) throw new Error(`Unexpected broad entitlement: ${key}.`);
  if (
    !local &&
    (actual['com.apple.application-identifier'] !== `${env.APP_STORE_TEAM_ID}.${identifier}` ||
      actual['com.apple.developer.team-identifier'] !== env.APP_STORE_TEAM_ID)
  )
    throw new Error('Signed team/app entitlements do not match APP_STORE_TEAM_ID.');
  if (!local && !existsSync(resolve(app, 'Contents/embedded.provisionprofile')))
    throw new Error('The distribution bundle has no embedded provisioning profile.');
  if (mode === 'package') {
    if (
      !/^(3rd Party Mac Developer Installer:|Mac Installer Distribution:)/.test(
        env.APP_STORE_INSTALLER_IDENTITY || '',
      )
    )
      throw new Error(
        'Set APP_STORE_INSTALLER_IDENTITY to your Mac App Store installer certificate identity.',
      );
    await execute(
      'productbuild',
      ['--component', app, '/Applications', '--sign', env.APP_STORE_INSTALLER_IDENTITY, pkg],
      env,
    );
    await execute('pkgutil', ['--check-signature', pkg], env);
  }
  console.log(`Verified sandbox entitlements: ${app}`);
  return { app, pkg };
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  workflow(process.argv[2] || 'build').catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
