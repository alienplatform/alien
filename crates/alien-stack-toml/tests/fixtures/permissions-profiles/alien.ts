import * as alien from "@alienplatform/core"

const reports = new alien.Storage("reports").build()
const archive = new alien.Storage("archive").build()
const cache = new alien.Kv("cache").build()
const jobs = new alien.Queue("jobs").build()
const secrets = new alien.Vault("secrets").build()

export default new alien.Stack("acme-permissions")
  .platforms(["aws", "gcp"])
  .add(reports, "frozen")
  .add(archive, "frozen")
  .add(cache, "frozen")
  .add(jobs, "frozen")
  .add(secrets, "frozen")
  .permissions({
    profiles: {
      reader: {
        "*": ["storage/data-read", "kv/data-read"],
        reports: ["storage/data-write"],
        cache: ["kv/data-write", "kv/data-read"],
      },
      processor: {
        "*": ["queue/data-read"],
        jobs: [
          "queue/data-write",
          {
            id: "acme/jobs-metrics",
            description: "Publish job metrics.",
            platforms: {
              aws: [
                {
                  grant: { actions: ["cloudwatch:PutMetricData"] },
                  binding: { stack: { resources: ["*"] } },
                },
              ],
            },
          },
        ],
        secrets: ["vault/data-read"],
      },
      archivist: {
        archive: ["storage/data-read", "storage/data-write"],
      },
      idle: {},
    },
  })
  .build()
