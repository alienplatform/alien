const required = (name: string): string => {
  const value = process.env[name]
  if (!value) {
    throw new Error(`Missing required environment variable ${name}`)
  }
  return value
}

const apiUrl = required("API_URL")
const appSecret = process.env.APP_SECRET
const intervalSeconds = Number(process.env.SCHEDULE_INTERVAL_SECONDS ?? "60")
const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))

async function enqueueMaintenance() {
  const headers = appSecret ? { "x-app-secret": appSecret } : undefined
  const response = await fetch(`${apiUrl}/internal/maintenance`, {
    method: "POST",
    headers,
  })
  if (!response.ok) {
    throw new Error(`maintenance enqueue failed with HTTP ${response.status}: ${await response.text()}`)
  }
  console.log(await response.text())
}

while (true) {
  try {
    await enqueueMaintenance()
  } catch (error) {
    console.warn("maintenance enqueue failed; retrying", error)
  }
  await sleep(intervalSeconds * 1000)
}

export {}
