const repository = require("./renovate-repository.json")
delete repository.$schema

// Mend-hosted Renovate reads renovate.json, which disables it. This process
// ignores that file and is the only Renovate run for the repository.
module.exports = {
  platform: "github",
  onboarding: false,
  requireConfig: "ignored",
  gitAuthor: "Renovate Bot <29139614+renovate[bot]@users.noreply.github.com>",
  binarySource: "global",
  executionTimeout: 45,
  allowedCommands: ["^bash scripts/repair-renovate-lockfiles.sh$"],
  repositories: [
    {
      repository: "alienplatform/alien",
      ...repository,
      enabled: true,
    },
  ],
}
