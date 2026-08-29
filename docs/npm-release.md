# npm releases

`@zlepper/kineprism` is distributed as one main npm package plus native packages for Linux
(glibc and musl on x64 and arm64), macOS (x64 and arm64), and Windows (x64 and arm64).
The native binaries are built by the normal `Tests` workflow. The publishing workflow only
packages binaries from that successful workflow run; it never recompiles them.

## One-time bootstrap

Trusted Publishing must be configured only after every generated package already exists on npm.
The initial publication is intentionally a temporary local procedure, not a repository script.

1. Commit and push the intended version. Wait for the `Tests` run for that exact commit to pass,
   including every `Build <target> release binary` job. Record its run ID and the full commit SHA.
2. Check out that SHA locally and install cargo-npm:

   ```console
   git checkout <full-commit-sha>
   cargo install --locked cargo-npm --version 0.1.4
   npm login
   ```

3. After interactive npm login succeeds, have the release coordinator perform the temporary
   artifact download, target-layout assembly, package validation, and
   `cargo npm publish` sequence. The command sequence uses only the binaries from that successful
   CI run and is deleted afterwards; no npm token or helper is committed. Prerelease versions use
   their leading prerelease identifier as the npm dist-tag (for example, `0.0.1-alpha.1` uses
   `alpha`); stable releases retain npm's default `latest` tag.
4. Confirm that all nine package names exist on npm, then configure Trusted Publishing for every
   one. npm requires 2FA and npm 11.15 or newer for `npm trust`:

   ```console
   npm --version
   npm trust github @zlepper/kineprism --repo zlepper/kineprism --file publish-npm.yml --allow-publish --yes
   npm trust github @zlepper/kineprism-linux-x64 --repo zlepper/kineprism --file publish-npm.yml --allow-publish --yes
   npm trust github @zlepper/kineprism-linux-x64-musl --repo zlepper/kineprism --file publish-npm.yml --allow-publish --yes
   npm trust github @zlepper/kineprism-linux-arm64 --repo zlepper/kineprism --file publish-npm.yml --allow-publish --yes
   npm trust github @zlepper/kineprism-linux-arm64-musl --repo zlepper/kineprism --file publish-npm.yml --allow-publish --yes
   npm trust github @zlepper/kineprism-darwin-x64 --repo zlepper/kineprism --file publish-npm.yml --allow-publish --yes
   npm trust github @zlepper/kineprism-darwin-arm64 --repo zlepper/kineprism --file publish-npm.yml --allow-publish --yes
   npm trust github @zlepper/kineprism-win32-x64 --repo zlepper/kineprism --file publish-npm.yml --allow-publish --yes
   npm trust github @zlepper/kineprism-win32-arm64 --repo zlepper/kineprism --file publish-npm.yml --allow-publish --yes
   ```

   Pause for two seconds between commands if npm rate limits requests. If the CLI path fails, use
   the npmjs.com Trusted Publisher UI for each package: provider **GitHub Actions**, repository
   `zlepper/kineprism`, workflow filename `publish-npm.yml`, and allowed action **npm publish**.
5. Dispatch **Publish npm package** once with the bootstrap SHA and version. cargo-npm recognizes
   the already-published packages; the workflow then creates the matching `v<version>` tag and
   GitHub Release with all native binaries attached.

No npm token is stored in GitHub Actions. If a temporary npm credential was created outside
`npm login`, revoke it after completing this process.

## Normal releases

1. Update the Cargo version, commit, and push it.
2. Wait for the complete `Tests` workflow for that commit to succeed.
3. Dispatch **Publish npm package**, entering the full tested commit SHA and its matching Cargo
   version without the `v` prefix.
4. The workflow downloads the exact CI artifacts, publishes every npm package through OIDC, and
   creates the corresponding Git tag and GitHub Release.

No npm login or token is involved in normal releases. If npm publication succeeds but GitHub
Release creation fails, rerun the dispatch with the same SHA and version after resolving the
GitHub failure; cargo-npm skips versions that already exist.
