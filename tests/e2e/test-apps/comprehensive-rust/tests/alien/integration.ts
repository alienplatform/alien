import { describe, it, expect, beforeAll } from 'vitest'
import { getResourceOutputs } from "@aliendotdev/core"
import { getStackState } from "@aliendotdev/core/tests"

const state = getStackState()
const functionUrl = getResourceOutputs({
  state,
  resource: { type: "function", name: "test-alien-function" }
}).url

describe('Alien Test Server Integration Tests', () => {
  beforeAll(() => {
    // Ensure the deployment outputs are available
    expect(functionUrl).toBeDefined()
  })

  describe('/hello endpoint', () => {
    it('should return hello message', async () => {
      const response = await fetch(`${functionUrl}/hello`)
      expect(response.status).toBe(200)

      const text = await response.text()
      expect(text).toBe('Hello from alien-runtime test server!')
    })
  })

  describe('/inspect endpoint', () => {
    it('should echo back the request body', async () => {

      const testPayload = {
        test_key: 'test_value',
        another_key: 123,
        nested: {
          data: true
        }
      }

      const response = await fetch(`${functionUrl}/inspect`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json'
        },
        body: JSON.stringify(testPayload)
      })

      expect(response.status).toBe(200)

      const result = await response.json()
      expect(result.success).toBe(true)
      expect(result.requestBody).toEqual(testPayload)
    })
  })

  describe('/env-var endpoint', () => {
    it('should read environment variables', async () => {
      const response = await fetch(`${functionUrl}/env-var/RUST_LOG`)
      expect(response.status).toBe(200)

      const result = await response.json()
      expect(result.success).toBe(true)
      expect(result.variable).toBe('RUST_LOG')
      expect(result.value).toBe('info')
    })

    it('should handle missing environment variables', async () => {
      const response = await fetch(`${functionUrl}/env-var/DOES_NOT_EXIST`)
      expect(response.status).toBe(200)

      const result = await response.json()
      expect(result.success).toBe(false)
      expect(result.error).toBe('Environment variable not found')
    })
  })

  describe('Storage integration', () => {
    it('should perform storage operations successfully', async () => {    
      const response = await fetch(`${functionUrl}/storage-test/test-alien-storage`, {
        method: 'POST'
      })

      expect(response.status).toBe(200)

      const result = await response.json()
      expect(result.overallSuccess).toBe(true)
      expect(result.bindingName).toBe('test-alien-storage')

      // Check that all operations succeeded
      const operations = result.operations
      expect(operations).toBeInstanceOf(Array)

      // Should have put, get, delete, and head_after_delete operations
      expect(operations.length).toBeGreaterThanOrEqual(4)

      for (const op of operations) {
        expect(op.success).toBe(true)
      }

      // Verify specific operations
      const putOp = operations.find((op: any) => op.operation === 'put')
      expect(putOp).toBeDefined()
      expect(putOp.success).toBe(true)

      const getOp = operations.find((op: any) => op.operation === 'get')
      expect(getOp).toBeDefined()
      expect(getOp.success).toBe(true)
      expect(getOp.dataMatch).toBe(true)
    })
  })

  describe('Server-Sent Events', () => {
    it('should stream SSE messages', async () => {
      const response = await fetch(`${functionUrl}/sse`)
      expect(response.status).toBe(200)
      expect(response.headers.get('content-type')).toContain('text/event-stream')

      // Read a few events from the stream
      const reader = response.body?.getReader()
      const decoder = new TextDecoder()

      if (reader) {
        let messagesReceived = 0
        const maxMessages = 3

        while (messagesReceived < maxMessages) {
          const { done, value } = await reader.read()
          if (done) break

          const chunk = decoder.decode(value)
          const lines = chunk.split('\n')

          for (const line of lines) {
            if (line.startsWith('data: ')) {
              const message = line.substring(6)
              expect(message).toMatch(/sse_message_\d+/)
              messagesReceived++
            }
          }
        }

        reader.cancel()
        expect(messagesReceived).toBeGreaterThan(0)
      }
    })
  })
}) 