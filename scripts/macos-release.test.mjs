import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createWorkflow } from './macos-release.mjs';

const identity = 'Developer ID Application: Test Team (ABCDEFGHIJ)';
const submissionId = '12345678-1234-1234-1234-123456789abc';
const signature = `Authority=${identity}\nTeamIdentifier=ABCDEFGHIJ\nflags=0x10000(runtime)\nTimestamp=Oct 3, 2026 at 2:00:00 PM\n`;

function fixture({
  status = 'Accepted',
  details = signature,
  gatekeeper = 'accepted\nsource=Notarized Developer ID',
  target,
} = {}) {
  const calls = [];
  const files = new Map();
  const workflow = createWorkflow({
    platform: 'darwin',
    target,
    env: {
      APPLE_SIGNING_IDENTITY: identity,
      APPLE_PASSWORD: 'synthetic-value',
      MACOS_NOTARY_PROFILE: 'Profile with spaces',
    },
    exists: () => true,
    write: (path, value) => files.set(path, value),
    read: (path) => files.get(path),
    log: () => {},
    run: async (command, args, options = {}) => {
      calls.push({ command, args, options });
      if (command === 'security') return { stdout: `1) FAKEHASH "${identity}"`, stderr: '' };
      if (command === 'codesign' && args[0] === '--display') return { stdout: '', stderr: details };
      if (command === 'spctl') return { stdout: '', stderr: gatekeeper };
      if (args[0] === 'notarytool' && args[1] === 'submit')
        return { stdout: JSON.stringify({ id: submissionId, status }), stderr: '' };
      return { stdout: '', stderr: '' };
    },
  });
  return { workflow, calls, files };
}

test('release signs, waits for Accepted, staples, assesses, and recreates the final ZIP', async () => {
  const { workflow, calls, files } = fixture({ target: 'universal-apple-darwin' });
  await workflow.release();
  const build = calls.find(({ command }) => command === process.execPath);
  assert.equal(build.options.env.APPLE_SIGNING_IDENTITY, identity);
  assert.equal(build.options.env.APPLE_PASSWORD, undefined);
  assert.deepEqual(build.args.slice(-2), ['--target', 'universal-apple-darwin']);
  const submit = calls.findIndex(({ args }) => args[0] === 'notarytool' && args[1] === 'submit');
  const staple = calls.findIndex(({ args }) => args[0] === 'stapler' && args[1] === 'staple');
  const assess = calls.findIndex(({ command }) => command === 'spctl');
  const zipIndexes = calls.flatMap(({ command }, index) => (command === 'ditto' ? [index] : []));
  assert.equal(zipIndexes.length, 2);
  assert.ok(zipIndexes[0] < submit && submit < staple && staple < assess && assess < zipIndexes[1]);
  assert.ok(calls[submit].args.includes('--wait'));
  assert.equal(
    calls[submit].args[calls[submit].args.indexOf('--keychain-profile') + 1],
    'Profile with spaces',
  );
  assert.ok(
    calls[submit].args.includes(workflow.paths.archive),
    'Path containing spaces stays one argument',
  );
  assert.ok(workflow.paths.archive.includes('universal-apple-darwin'));
  assert.equal(JSON.parse(files.get(workflow.paths.resultPath)).id, submissionId);
});

test('Invalid submission status stops stapling even if notarytool exits successfully', async () => {
  const { workflow, calls, files } = fixture({ status: 'Invalid' });
  await assert.rejects(workflow.release(), /Apple returned Invalid/);
  assert.equal(JSON.parse(files.get(workflow.paths.resultPath)).id, submissionId);
  assert.ok(!calls.some(({ args }) => args[0] === 'stapler'));
  await workflow.log();
  assert.ok(
    calls.some(
      ({ args }) => args[0] === 'notarytool' && args[1] === 'log' && args[2] === submissionId,
    ),
  );
});

test('ad-hoc builds are rejected before packaging or upload', async () => {
  const { workflow, calls } = fixture({
    details: 'Signature=adhoc\nTeamIdentifier=not set\nflags=0x10000(runtime)',
  });
  await assert.rejects(workflow.notarize(), /Developer ID Application certificate/);
  assert.ok(!calls.some(({ command }) => command === 'ditto'));
  assert.ok(!calls.some(({ args }) => args[0] === 'notarytool'));
});

test('disabled Gatekeeper is not treated as successful release verification', async () => {
  const { workflow, calls } = fixture({ gatekeeper: 'assessments disabled' });
  await assert.rejects(workflow.staple(), /Gatekeeper did not report acceptance/);
  assert.ok(!calls.some(({ command }) => command === 'ditto'));
});

test('missing signing identity fails before any build or credential access', async () => {
  let ran = false;
  const workflow = createWorkflow({
    platform: 'darwin',
    env: {},
    run: async () => {
      ran = true;
    },
  });
  await assert.rejects(workflow.build(), /APPLE_SIGNING_IDENTITY/);
  assert.equal(ran, false);
});
