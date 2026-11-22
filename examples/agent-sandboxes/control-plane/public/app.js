// Plain browser JS: threads on the left, the selected thread's events, Approve / Deny.

const $ = id => document.getElementById(id)
let current = null
let source = null

async function api(path, body) {
  const response = await fetch(path, body ? { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) } : {})
  const json = await response.json()
  if (!response.ok) throw new Error(json.error || response.statusText)
  return json
}

function line(cls, text) {
  const div = document.createElement("div")
  div.className = `msg ${cls}`
  div.textContent = text
  $("log").append(div)
  $("log").scrollTop = $("log").scrollHeight
}

// Claude Code's stream-json events, reduced to what a chat shows.
function showAgent(event) {
  if (event.type === "assistant") {
    for (const block of event.message?.content ?? []) {
      if (block.type === "text" && block.text.trim()) line("assistant", block.text)
      if (block.type === "tool_use") line("tool", `$ ${block.input?.command ?? JSON.stringify(block.input)}`)
    }
  } else if (event.type === "user") {
    for (const block of event.message?.content ?? []) {
      if (block.type === "tool_result") {
        const text = Array.isArray(block.content) ? block.content.map(c => c.text ?? "").join("") : String(block.content ?? "")
        line("tool", text.slice(0, 2000))
      }
    }
  } else if (event.type === "system" && event.subtype === "init") {
    line("notice", `agent session ${event.session_id}`)
  } else if (event.type === "result") {
    line("notice", `turn done in ${Math.round((event.duration_ms ?? 0) / 1000)}s`)
  }
}

function showApprovals(pending) {
  const box = $("approvals")
  box.replaceChildren()
  for (const write of pending) {
    const div = document.createElement("div")
    div.className = "approval"
    const text = document.createElement("span")
    text.textContent = `${write.user} wants ${write.method} ${write.path} ${JSON.stringify(write.body)}`
    const approve = document.createElement("button")
    approve.textContent = "Approve"
    approve.onclick = () => api(`/api/threads/${current}/approvals/${write.id}`, { approve: true, by: $("user").value })
    const deny = document.createElement("button")
    deny.textContent = "Deny"
    deny.onclick = () => api(`/api/threads/${current}/approvals/${write.id}`, { approve: false, by: $("user").value })
    div.append(text, approve, deny)
    box.append(div)
  }
}

function show(event) {
  if (event.type === "user") line("user", `${event.user}: ${event.text}`)
  else if (event.type === "agent") showAgent(event.event)
  else if (event.type === "notice") line("notice", event.text)
  else if (event.type === "state") { $("state").textContent = event.state; loadThreads() }
  else if (event.type === "approvals") showApprovals(event.pending)
  else if (event.type === "exec") line("exec", `$ ${event.command}\n${event.stdout}${event.stderr}[exit ${event.exitCode}]`)
}

async function loadThreads() {
  const { threads } = await api("/api/threads")
  const list = $("threads")
  list.replaceChildren()
  for (const t of threads) {
    const b = document.createElement("button")
    b.className = `thread${t.id === current ? " active" : ""}`
    b.innerHTML = `<div></div><div class="state"></div>`
    b.children[0].textContent = t.title
    b.children[1].textContent = `${t.user} · ${t.state}`
    b.onclick = () => select(t)
    list.append(b)
  }
}

function select(thread) {
  current = thread.id
  $("title").textContent = thread.title
  $("state").textContent = thread.state
  $("user").value = thread.user
  $("log").replaceChildren()
  $("approvals").replaceChildren()
  source?.close()
  source = new EventSource(`/api/threads/${thread.id}/events`)
  source.onmessage = message => show(JSON.parse(message.data))
  loadThreads()
}

$("new").onclick = async () => {
  const title = prompt("Thread title", "Orders") || "Orders"
  select(await api("/api/threads", { title, user: $("user").value }))
}

$("send").onsubmit = async event => {
  event.preventDefault()
  if (!current || !$("text").value) return
  const text = $("text").value
  $("text").value = ""
  try { await api(`/api/threads/${current}/messages`, { user: $("user").value, text }) } catch (e) { line("notice", e.message) }
}

$("exec").onsubmit = async event => {
  event.preventDefault()
  if (!current || !$("command").value) return
  const command = $("command").value
  $("command").value = ""
  try { await api(`/api/threads/${current}/exec`, { command }) } catch (e) { line("notice", e.message) }
}

loadThreads()
