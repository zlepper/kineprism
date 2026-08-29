# npm releases

`@zlepper/kineprism` is distributed as one main npm package plus native packages for Linux
(glibc and musl on x64 and arm64), macOS (x64 and arm64), and Windows (x64 and arm64). Trusted
Publishing publishes all nine packages from the tag-triggered GitHub Actions workflow.

## Normal releases

1. Update the Cargo version, commit it, and push the commit.
2. Create and push a matching `v<version>` tag, for example `v0.0.1-alpha.2`.
3. The **Publish npm package** workflow validates that the tag version matches Cargo metadata,
   builds the complete native-binary matrix, publishes every npm package through OIDC, and creates
   the GitHub Release with the release binaries attached.

