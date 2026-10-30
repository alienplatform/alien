import { describe, expect, it } from "vitest"
import { Queue } from "../queue.js"
import { Storage } from "../storage.js"

describe("Storage.notify", () => {
  it("declares independent rules with default events and literal filters", () => {
    const queue = new Queue("work").build()
    const storage = new Storage("objects")
      .notify(queue, { prefix: "in/*/é/", suffix: ".json" })
      .notify(queue)
      .build()
    expect(storage.config).toMatchObject({
      type: "storage",
      notifications: [
        { queue: queue.ref(), events: ["created"], prefix: "in/*/é/", suffix: ".json" },
        { queue: queue.ref(), events: ["created"] },
      ],
    })
  })

  it("copies event options so later caller changes cannot alter a declaration", () => {
    const events: "created"[] = ["created"]
    const builder = new Storage("objects").notify(new Queue("work").build(), { events })
    events.length = 0
    expect(builder.build().config).toMatchObject({ notifications: [{ events: ["created"] }] })
  })

  it("rejects non-queue resources and more than ten rules", () => {
    expect(() => new Storage("objects").notify(new Storage("wrong").build())).toThrow(
      "expected a Queue",
    )
    const builder = new Storage("objects")
    const queue = new Queue("work").build()
    for (let index = 0; index < 10; index++) builder.notify(queue)
    expect(builder.build().config).toHaveProperty("notifications.length", 10)
    builder.notify(queue)
    expect(() => builder.build()).toThrow()
  })

  it("leaves existing storage declarations unchanged", () => {
    expect(new Storage("objects").build().config).toEqual({
      type: "storage",
      id: "objects",
      publicRead: false,
      versioning: false,
      lifecycleRules: [],
      corsAllowedOrigins: [],
    })
  })
})
