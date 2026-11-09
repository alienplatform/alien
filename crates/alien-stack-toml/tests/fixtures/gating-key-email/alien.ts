import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  mail_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    label: "Send mail",
    description: "Whether this deployment gets the mail sender and the key that protects it.",
    default: false,
  }),
})

const inbox = new alien.Storage("inbox").build()

const notifications = new alien.Email("notifications")
  .domains(["mail.acme.example"])
  .inbound(inbox)
  .enabled(inputs.mail_enabled)
  .build()

const signing = new alien.Key("signing").enabled(inputs.mail_enabled).build()

export default new alien.Stack("acme-gating-key-email")
  .platforms(["aws", "gcp"])
  .inputs(inputs)
  .add(inbox, "frozen")
  .add(notifications, "frozen")
  .add(signing, "frozen")
  .build()
