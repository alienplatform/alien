import * as alien from "@alienplatform/core"

const failed = new alien.Queue("failed").build()
const tasks = new alien.Queue("tasks").deadLetter(failed, { maxAttempts: 7 }).build()

export default new alien.Stack("acme-dead-letter")
  .platforms(["aws", "gcp", "azure"])
  .add(failed, "frozen")
  .add(tasks, "frozen")
  .build()
