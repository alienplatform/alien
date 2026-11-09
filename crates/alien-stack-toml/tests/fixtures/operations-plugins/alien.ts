import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  db_url: alien.string({
    required: true,
    providedBy: "deployer",
    label: "Database URL",
    description: "Analytics database URL.",
  }),
  db_password: alien.secret({
    required: true,
    providedBy: "deployer",
    label: "Database password",
    description: "Analytics database password.",
  }),
})

const exports = new alien.Storage("exports").build()
const archive = new alien.Storage("archive").build()

const stack = new alien.Stack("acme-operations")
  .inputs(inputs)
  .platforms(["aws"])
  .add(exports, "frozen")
  .add(archive, "frozen")
  .operations({
    kubernetes: { approval: { "*": "auto" } },
    clickhouse: {
      url: inputs.db_url,
      password: inputs.db_password,
      database: "app",
      approval: {
        read: "auto",
        write: { decision: "manual", maxRisk: "medium" },
        drop: { decision: "manual" },
      },
    },
    s3: { buckets: [exports, archive], "default-bucket": exports, region: "us-east-1" },
  })
  .build()

// The builders cannot write an `env` setting, which reads a variable at deploy time. Set it directly.
stack.operations!.plugins!.clickhouse.settings!.token = { env: "CLICKHOUSE_TOKEN" }

export default stack
