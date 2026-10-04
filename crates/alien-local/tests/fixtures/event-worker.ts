import { appendFileSync } from "node:fs"
import { onCronEvent, onStorageEvent } from "../../../../packages/sdk/src/worker-runtime/registry"
import { runWorker } from "../../../../packages/sdk/src/worker-runtime/index"

const events: string[] = []
let cron = 0
appendFileSync(process.env.LAUNCH_RECORD!, `${process.env.ROLE}:${process.pid}\n`)
onStorageEvent("*", async event => {
  events.push(event.objectKey)
})
onCronEvent("*", async () => {
  cron++
})
await runWorker({
  fetch() {
    return Response.json({ role: process.env.ROLE, pid: process.pid, events, cron })
  },
})
