import { test } from 'node:test';
import assert from 'node:assert/strict';
import { validateProfile, validateSignature } from './appstore.mjs';
const team = 'ABCDE12345';
const profile = () => ({
  TeamIdentifier: [team],
  ExpirationDate: '2030-01-01',
  Entitlements: {
    'com.apple.application-identifier': `${team}.labs.rippley.chip-count`,
    'com.apple.developer.team-identifier': team,
  },
});
test('distribution profile must match app, team and expiration', () => {
  assert.doesNotThrow(() => validateProfile(profile(), team));
  assert.throws(() => validateProfile(profile(), 'OTHER12345'), /team/);
  const wrong = profile();
  wrong.Entitlements['com.apple.application-identifier'] += '.other';
  assert.throws(() => validateProfile(wrong, team), /authorize/);
  const expired = profile();
  expired.ExpirationDate = '2020-01-01';
  assert.throws(() => validateProfile(expired, team), /expired/);
  expired.ExpirationDate = 'invalid';
  assert.throws(() => validateProfile(expired, team), /expired/);
});
test('development and Developer ID profiles cannot enter App Store packaging', () => {
  for (const field of ['ProvisionedDevices', 'ProvisionsAllDevices']) {
    assert.throws(
      () =>
        validateProfile(
          { ...profile(), [field]: field === 'ProvisionedDevices' ? ['device'] : true },
          team,
        ),
      /distribution profile/,
    );
  }
  const debug = profile();
  debug.Entitlements['get-task-allow'] = true;
  assert.throws(() => validateProfile(debug, team), /distribution profile/);
});
test('ad hoc sandbox test signatures never pass distribution verification', () => {
  const app = 'Identifier=labs.rippley.chip-count\n';
  assert.doesNotThrow(() => validateSignature(app + 'Authority=Apple Distribution: Example\n'));
  assert.doesNotThrow(() =>
    validateSignature('Identifier=labs.rippley.chip-count.sandbox-test\nSignature=adhoc\n', true),
  );
  assert.throws(
    () =>
      validateSignature(
        'Identifier=labs.rippley.chip-count.sandbox-test\nAuthority=Apple Distribution: Example\n',
      ),
    /bundle identifier/,
  );
  assert.throws(() => validateSignature(app + 'Signature=adhoc\n'), /distribution signature/);
  assert.throws(
    () => validateSignature(app + 'Authority=Developer ID Application: Example\n'),
    /distribution signature/,
  );
});
