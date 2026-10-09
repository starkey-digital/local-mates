# local mates

Public repo: keep infrastructure details (IPs, cluster layout, secrets, values) out of it. Deployment values live in the private GitOps repo.

## Release

- Bump `version` in `Cargo.toml` (and `cargo update -p local-mates` for the lockfile) in the release-notes commit; the Release workflow fails if the tag doesn't match.
- Tags are `v`-prefixed (`v0.2.0`).
- Pushing the tag runs `.github/workflows/release.yml`, which creates the GitHub release itself (via `vpk upload`) and publishes the server images and Helm chart to GHCR. Do **not** run `gh release create`; once the workflow finishes, attach the notes with `gh release edit <tag> --notes-file changelog/RELEASE_<version>.md`.
- Link issues/PRs in changelog entries.
