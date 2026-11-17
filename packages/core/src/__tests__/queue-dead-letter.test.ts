import { describe, expect, it } from "vitest"
import { Queue } from "../queue.js"
import { Storage } from "../storage.js"

describe("Queue.deadLetter", () => {
  it("names the target queue and defaults to five attempts", () => {
    const failed = new Queue("failed").build()
    const tasks = new Queue("tasks").deadLetter(failed).build()
    expect(tasks.config).toEqual({
      type: "queue",
      id: "tasks",
      deadLetter: { queue: failed.ref(), maxAttempts: 5 },
    })
  })

  it("takes maxAttempts and keeps the last declaration", () => {
    const failed = new Queue("failed").build()
    const other = new Queue("other").build()
    const tasks = new Queue("tasks")
      .deadLetter(failed, { maxAttempts: 100 })
      .deadLetter(other, { maxAttempts: 7 })
      .build()
    expect(tasks.config).toMatchObject({
      deadLetter: { queue: other.ref(), maxAttempts: 7 },
    })
  })

  it("rejects a target that is not a queue", () => {
    expect(() => new Queue("tasks").deadLetter(new Storage("wrong").build())).toThrow(
      "expected a Queue",
    )
  })

  it("leaves a queue without a dead letter unchanged", () => {
    expect(new Queue("tasks").build().config).toEqual({ type: "queue", id: "tasks" })
  })
})
