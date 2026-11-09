import * as alien from "@alienplatform/core"

const inbox = new alien.Storage("inbox").build()
const mailEvents = new alien.Queue("mail-events").build()

const notifications = new alien.Email("notifications")
  .displayName("Notification mail")
  .description("Sends product mail and receives replies.")
  .domains(["mail.acme.example", "alerts.acme.example"])
  .inbound(inbox)
  .events(mailEvents)
  .build()

const digests = new alien.Email("digests")
  .domain("digest.acme.example")
  .inbound(inbox)
  .events(mailEvents)
  .build()

export default new alien.Stack("acme-email")
  .platforms(["aws"])
  .add(inbox, "frozen")
  .add(mailEvents, "frozen")
  .add(notifications, "frozen", { remoteAccess: true })
  .add(digests, "frozen")
  .build()
