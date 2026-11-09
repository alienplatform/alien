import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  audit_url: alien.string({
    providedBy: "deployer",
    required: true,
    label: "Audit URL",
    description: "Where the audit plugin reports.",
  }),
})

const reports = new alien.Storage("reports").build()

const stack = new alien.Stack("acme-custom-operations")
  .inputs(inputs)
  .platforms(["aws"])
  .add(reports, "frozen")
  .operations({
    plugins: [
      {
        name: "acme-audit",
        version: "0.3.1",
        sink: inputs.audit_url,
        bucket: reports,
        mode: "strict",
        approval: { "*": "manual", inspect: { decision: "auto", maxRisk: "low" } },
      },
      { name: "acme-cleanup", version: "2.0.0", approval: { "*": "auto" } },
    ],
  })
  .build()

// The builders cannot write an `env` setting, which reads a variable at deploy time. Set it directly.
stack.operations!.custom![0].settings!.token = { env: "AUDIT_TOKEN" }

export default stack
