import { useState } from "react"
import { createFileRoute, useRouter } from "@tanstack/react-router"
import "./styles.css"

type Issue = {
  id: string
  title: string
  body: string
  status: string
  created_at: string
}

const apiBase = import.meta.env.VITE_API_BASE ?? "/api"

async function loadIssues(): Promise<Issue[]> {
  const response = await fetch(`${apiBase}/issues`)
  if (!response.ok) {
    throw new Error(`failed to load issues: ${response.status}`)
  }
  const payload = (await response.json()) as { issues: Issue[] }
  return payload.issues
}

export const Route = createFileRoute("/")({
  component: Home,
  loader: loadIssues,
})

function Home() {
  const router = useRouter()
  const issues = Route.useLoaderData()
  const [title, setTitle] = useState("")
  const [body, setBody] = useState("")
  const [submitting, setSubmitting] = useState(false)

  async function createIssue() {
    setSubmitting(true)
    try {
      const response = await fetch(`${apiBase}/issues`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ title, body }),
      })
      if (!response.ok) {
        throw new Error(await response.text())
      }
      setTitle("")
      setBody("")
      await router.invalidate()
    } finally {
      setSubmitting(false)
    }
  }

  async function processIssue(id: string) {
    const response = await fetch(`${apiBase}/issues/${id}/process`, { method: "POST" })
    if (!response.ok) {
      throw new Error(await response.text())
    }
    await router.invalidate()
  }

  return (
    <main className="shell">
      <section className="panel">
        <div>
          <p className="eyebrow">Support desk</p>
          <h1>Open issues</h1>
        </div>
        <form
          className="new-issue"
          onSubmit={event => {
            event.preventDefault()
            void createIssue()
          }}
        >
          <input
            aria-label="Issue title"
            placeholder="Issue title"
            value={title}
            onChange={event => setTitle(event.target.value)}
            required
          />
          <textarea
            aria-label="Issue details"
            placeholder="Details"
            value={body}
            onChange={event => setBody(event.target.value)}
            required
          />
          <button disabled={submitting} type="submit">
            Create
          </button>
        </form>
      </section>

      <section className="issues">
        {issues.map(issue => (
          <article className="issue" key={issue.id}>
            <div>
              <span className="status">{issue.status}</span>
              <h2>{issue.title}</h2>
              <p>{issue.body}</p>
            </div>
            <button type="button" onClick={() => void processIssue(issue.id)}>
              Process
            </button>
          </article>
        ))}
      </section>
    </main>
  )
}
