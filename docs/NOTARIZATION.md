# Sign and notarize Chip Count for macOS

These scripts prepare a ZIP for direct distribution outside the Mac App Store. The app keeps its existing bundle identifier, `labs.rippley.chip-count`. Signing, notarization, and the stapled Apple ticket are required before calling a release ready.

## One-time setup

1. Enroll in the [Apple Developer Program](https://developer.apple.com/programs/enroll/). Membership is currently US$99 per year, with regional pricing and eligible fee waivers. A free development account cannot notarize a public release.
2. Install a current Xcode from Apple, open it once, and complete its setup. If needed, select it for command-line tools:

   ```sh
   sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
   npm run macos:doctor
   ```

   The doctor prints the selected developer directory, locations of `notarytool` and `stapler`, and available signing identities. It does not submit an app or prompt for credentials.
3. Create a **Developer ID Application** certificate. In Keychain Access, use Certificate Assistant → Request a Certificate From a Certificate Authority, and save the CSR to disk. In [Apple Certificates, Identifiers & Profiles](https://developer.apple.com/account/resources/certificates/list), create Developer ID Application, upload the CSR, download the certificate, and open the `.cer` to install it. The certificate must be paired with the private key created on this Mac. Apple's [certificate instructions](https://developer.apple.com/help/account/certificates/create-developer-id-certificates/) explain the Account Holder requirement. This workflow does not use an Apple Development, Apple Distribution, or Developer ID Installer certificate.

   If the doctor already shows a valid Developer ID Application identity, use that certificate and skip creating another. A sandboxed command may be unable to see your login Keychain; run the doctor in your ordinary terminal to confirm.
4. Run `npm run macos:doctor` again and copy the entire Developer ID Application identity into your shell:

   ```sh
   export APPLE_SIGNING_IDENTITY='Developer ID Application: Your Name or Company (YOURTEAMID)'
   ```

   Replace the example with the exact identity from your Keychain. This is public certificate metadata, not a password. Set it in each new terminal session, or keep this export in your shell configuration.
5. At [account.apple.com](https://account.apple.com/), sign in to your Apple Account and generate an **app-specific password** under Sign-In and Security. Have your developer Team ID available from your [developer membership details](https://developer.apple.com/account/).
6. Store notarization credentials in Keychain:

   ```sh
   npm run macos:credentials
   ```

   `notarytool` prompts for the Apple ID, developer Team ID, and app-specific password, validates them with Apple, and saves the profile as `chip-count-notary`. Enter the app-specific password at the secure prompt. Passwords are not written to repository files or package scripts. If asked to choose an authentication method, use the Apple ID/app-specific-password method. App Store Connect API-key authentication is also supported by Apple's interactive tool.

   To use a different profile name, set `MACOS_NOTARY_PROFILE` before both the credentials command and release commands:

   ```sh
   export MACOS_NOTARY_PROFILE='rippley-labs-notary'
   ```

## Release

From the repository root, install dependencies and run:

```sh
npm ci
npm run macos:release
```

The default builds for the current Mac architecture. On this development Mac that is Apple Silicon. The script:

1. Requires an installed Developer ID Application identity and builds through Tauri with hardened runtime enabled. Tauri signs the bundle and executable; raw notarization environment variables are removed from the build subprocess so submission happens through the explicit Keychain workflow.
2. Verifies the bundle's signature, developer Team ID, hardened runtime, and secure timestamp.
3. Creates a ZIP preserving the macOS bundle with `ditto` and submits it to `notarytool` using the Keychain profile, waiting for Apple to finish.
4. Saves `notarization-result.json` in the bundle output directory. Only an **Accepted** status permits the release workflow to continue.
5. Staples the ticket to the `.app`, validates it, and requires a successful Gatekeeper assessment reporting `Notarized Developer ID`.
6. Recreates the ZIP from the stapled app and prints **Release ready** with the file path.

For a default native build, distribute:

```text
target/release/bundle/macos/Chip Count-macOS.zip
```

Apple cannot staple a ticket to a ZIP itself. The script staples the app inside it and then recreates the ZIP. Do not rebuild or modify the app between submission and stapling; any changed app must be signed and notarized again. Close the development app before rebuilding its bundle.

## Universal build: Apple Silicon and Intel

Install both Rust targets once, then build and notarize both architectures in one app:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm run macos:release:universal
```

The universal ZIP is:

```text
target/universal-apple-darwin/release/bundle/macos/Chip Count-macOS.zip
```

If `CARGO_TARGET_DIR` is set, output is placed there instead of `target`. `CARGO_BUILD_TARGET` is also honored. Every separate step must use the same target and environment as the build.

## Individual commands and troubleshooting

| Command | What it does |
| --- | --- |
| `npm run macos:doctor` | Show Apple tools, signing identities, profile name, and output paths. |
| `npm run macos:credentials` | Interactively validate and save notarization credentials in Keychain. |
| `npm run macos:build` | Build and sign the app; does not submit it to Apple. |
| `npm run macos:package` | Verify Developer ID signing and package the app; the ZIP is not necessarily notarized yet. |
| `npm run macos:notarize` | Package the current signed app, upload to Apple, wait, and require Accepted. |
| `npm run macos:staple` | Staple, verify, and recreate the distributable ZIP. |
| `npm run macos:verify` | Verify signature, stapled ticket, and Gatekeeper acceptance. |
| `npm run macos:notarization:log` | Download Apple's diagnostic log for the saved submission ID. |
| `npm run macos:release` | Complete native build/sign/notarize/staple/verify/package workflow. |
| `npm run macos:release:universal` | Complete workflow for an Apple Silicon + Intel universal app. |
| `npm run test:release` | Run workflow regression tests with mocked Apple tools; no submission or credentials needed. |

Examples for separate universal steps and a specific submission:

```sh
npm run macos:build -- --target universal-apple-darwin
npm run macos:notarize -- --target universal-apple-darwin
npm run macos:staple -- --target universal-apple-darwin
npm run macos:notarization:log -- --target universal-apple-darwin
npm run macos:notarization:log -- YOUR_SUBMISSION_UUID
```

An **Invalid** result is saved even when `notarytool` exits successfully. Download `notarization-log.json`, fix the specific Apple-reported issue, rebuild, and submit again. A missing identity means the certificate/private-key pair is unavailable or expired; an authentication failure means the named Keychain profile or developer team needs attention. Notarization can take several minutes and requires network access. If the submission command is interrupted or times out, use Apple's `notarytool history` and `notarytool info` with the same `--keychain-profile` to recover its status before resubmitting.

Test the final downloaded ZIP on another Mac with ordinary Gatekeeper settings, ideally offline to check the stapled ticket. Notarization verifies Apple's security checks; it does not publish your application or submit it to the Mac App Store.

References: [Tauri macOS signing](https://v2.tauri.app/distribute/sign/macos/), [Apple Developer ID](https://developer.apple.com/developer-id/), [Apple notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).
