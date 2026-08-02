"use client"

import { Spinner } from "./spinner"

const PREVIEW_ROWS = 5

export type QueryInput = { question?: string; plan?: string; status?: string }
export type QueryOutput = {
  rowCount?: number | null
  rows?: Record<string, unknown>[]
  error?: string
}

export function QueryCard({
  state,
  input,
  output,
  errorText,
}: {
  state: "input-streaming" | "input-available" | "output-available" | "output-error"
  input?: QueryInput
  output?: QueryOutput
  errorText?: string
}) {
  const running = state === "input-streaming" || state === "input-available"
  const failure =
    state === "output-error" ? (errorText ?? "The query could not be run.") : output?.error
  const call = input?.question && describe(input)
  const rows = output?.rows ?? []
  const columns = rows.length > 0 ? Object.keys(rows[0]) : []
  const numeric = new Set(columns.filter(column => typeof rows[0]?.[column] === "number"))

  return (
    <div className="my-3 overflow-hidden rounded-xl border border-white/40 bg-card/80 backdrop-blur-sm">
      <div className="flex items-center gap-2 border-b border-edge px-4 py-2">
        <DatabaseIcon />
        <span
          className={`font-mono text-[11px] uppercase tracking-widest ${failure ? "text-red-400" : "text-zinc-400"}`}
        >
          {running ? "Querying the database" : failure ? "Query failed" : "Queried the database"}
        </span>
        {running && <Spinner className="text-[11px] text-brand" />}
        {typeof output?.rowCount === "number" && (
          <span className="ml-auto font-mono text-[11px] tabular-nums text-zinc-400">
            {output.rowCount} {output.rowCount === 1 ? "row" : "rows"}
          </span>
        )}
      </div>

      {call && (
        <pre className="overflow-x-auto bg-white/[0.03] px-4 py-2.5 font-mono text-[13px] leading-[1.7] text-zinc-100">
          {call}
        </pre>
      )}

      {failure && <div className="px-4 py-2.5 text-sm text-red-400">{failure}</div>}

      {columns.length > 0 && (
        <div className="overflow-x-auto">
          <table className="w-full border-collapse font-mono text-[11px]">
            <thead>
              <tr className="border-b border-edge text-left text-zinc-400">
                {columns.map(column => (
                  <th
                    key={column}
                    className={`px-4 py-1.5 font-medium ${numeric.has(column) ? "text-right" : ""}`}
                  >
                    {column}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.slice(0, PREVIEW_ROWS).map((row, i) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: static preview slice with no stable row id
                <tr key={i} className="border-b border-edge/50 last:border-0">
                  {columns.map(column => (
                    <td
                      key={column}
                      className={`px-4 py-1.5 tabular-nums text-zinc-100 ${numeric.has(column) ? "text-right" : ""}`}
                    >
                      {String(row[column] ?? "")}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
          {rows.length > PREVIEW_ROWS && (
            <div className="px-4 py-1.5 font-mono text-[10px] text-zinc-400">
              +{rows.length - PREVIEW_ROWS} more
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function describe({ question, plan, status }: QueryInput): string {
  const filters = [plan && `plan=${plan}`, status && `status=${status}`].filter(Boolean)
  return [question, ...filters].join("  ·  ")
}

function DatabaseIcon() {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className="text-zinc-400"
    >
      <ellipse cx="12" cy="5" rx="9" ry="3" />
      <path d="M3 5v14a9 3 0 0 0 18 0V5" />
      <path d="M3 12a9 3 0 0 0 18 0" />
    </svg>
  )
}
