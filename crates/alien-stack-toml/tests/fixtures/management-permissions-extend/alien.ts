import * as alien from "@alienplatform/core"

const reports = new alien.Storage("reports").build()
const jobs = new alien.Queue("jobs").build()

export default new alien.Stack("acme-management-extend")
  .platforms(["aws", "gcp"])
  .add(reports, "frozen")
  .add(jobs, "frozen")
  .permissions({
    profiles: {},
    management: {
      extend: {
        "*": ["storage/management", "queue/management"],
        reports: ["storage/data-read"],
        jobs: [
          "queue/data-read",
          {
            id: "acme/jobs-audit",
            description: "Audit the jobs queue.",
            platforms: {
              gcp: [
                {
                  grant: { permissions: ["pubsub.topics.get"] },
                  binding: { stack: { scope: "projects/${projectId}" } },
                },
              ],
            },
          },
        ],
      },
    },
  })
  .build()
