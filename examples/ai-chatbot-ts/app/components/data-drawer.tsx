"use client"

import { useEffect, useRef, useState } from "react"
import { Spinner } from "./spinner"

type Table = { name: string; rows: Record<string, unknown>[]; total: number }

/** The tables the model queries, so an answer can be checked against the data. */
export function DataDrawer() {
  const [open, setOpen] = useState(false)
  const [tables, setTables] = useState<Table[] | null>(null)
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    if (!open || tables || failed) return
    fetch("/api/tables")
      .then(r => r.json())
      .then((d: { tables: Table[] }) => setTables(d.tables))
      .catch(() => setFailed(true))
  }, [open, tables, failed])

  // showModal() is what puts the dialog in the top layer, above every stacking
  // context on the page, and brings Escape and the backdrop with it.
  const dialogRef = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    const dialog = dialogRef.current
    if (!dialog) return
    if (open && !dialog.open) dialog.showModal()
    if (!open && dialog.open) dialog.close()
  }, [open])

  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="cursor-pointer rounded-full border border-edge px-3 py-1.5 font-mono text-xs text-zinc-100 transition-colors hover:bg-white/5 hover:text-white outline-none focus-visible:ring-2 focus-visible:ring-brand/50"
      >
        See the data
      </button>

      {/* biome-ignore lint/a11y/useKeyWithClickEvents: Escape is the keyboard path, via onCancel */}
      <dialog
        ref={dialogRef}
        aria-label="The demo tables"
        onClose={() => setOpen(false)}
        onCancel={() => setOpen(false)}
        // The backdrop belongs to the dialog, so a click on it targets the dialog
        // itself; anything inside targets a child.
        onClick={e => e.target === dialogRef.current && setOpen(false)}
        className="my-0 ml-auto mr-0 h-dvh max-h-none w-full max-w-lg bg-transparent p-0 backdrop:bg-black/60 backdrop:backdrop-blur-[2px] open:motion-safe:animate-[slide-in_220ms_cubic-bezier(0.32,0.72,0,1)]"
      >
        {open && (
          <div className="flex h-full flex-col border-l border-edge bg-zinc-950/95 backdrop-blur-xl">
            <header className="flex items-start gap-3 border-b border-edge px-5 py-4">
              <div>
                <h2 className="text-sm font-medium tracking-tight text-zinc-100">
                  The data behind the answers
                </h2>
                <p className="mt-1 text-xs leading-relaxed text-zinc-400">
                  Read from the stack's private Postgres through the same read-only connection the
                  model's tool uses.
                </p>
              </div>
              <button
                type="button"
                onClick={() => setOpen(false)}
                aria-label="Close"
                className="ml-auto flex size-7 shrink-0 cursor-pointer items-center justify-center rounded-full text-zinc-400 transition-colors hover:bg-white/5 hover:text-zinc-100 outline-none focus-visible:ring-2 focus-visible:ring-brand/50"
              >
                <CloseIcon />
              </button>
            </header>

            <div className="flex-1 space-y-4 overflow-y-auto p-5">
              {failed && (
                <p className="font-mono text-[11px] text-red-400">Could not read the tables.</p>
              )}
              {!tables && !failed && (
                <div className="flex items-center gap-2 font-mono text-[11px] text-zinc-400">
                  <Spinner className="text-[11px] text-brand" />
                  Reading
                </div>
              )}
              {tables?.map(table => (
                <TableCard key={table.name} table={table} />
              ))}
            </div>
          </div>
        )}
      </dialog>
    </>
  )
}

// A `date` column arrives as a full ISO timestamp once it has been through JSON;
// the day is the only part the demo data carries.
function cell(value: unknown): string {
  const text = String(value ?? "")
  return /^\d{4}-\d{2}-\d{2}T00:00:00/.test(text) ? text.slice(0, 10) : text
}

function TableCard({ table }: { table: Table }) {
  const columns = table.rows.length > 0 ? Object.keys(table.rows[0]) : []
  const numeric = new Set(columns.filter(c => typeof table.rows[0]?.[c] === "number"))

  return (
    <div className="overflow-hidden rounded-xl border border-edge bg-card/60">
      <div className="flex items-center gap-2 border-b border-edge px-4 py-2">
        <span className="font-mono text-[11px] uppercase tracking-widest text-brand">
          {table.name}
        </span>
        <span className="ml-auto font-mono text-[11px] tabular-nums text-zinc-400">
          {table.total} {table.total === 1 ? "row" : "rows"}
        </span>
      </div>
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
            {table.rows.map(row => (
              <tr key={String(row.id)} className="border-b border-edge/50 last:border-0">
                {columns.map(column => (
                  <td
                    key={column}
                    className={`whitespace-nowrap px-4 py-1.5 tabular-nums text-zinc-100 ${numeric.has(column) ? "text-right" : ""}`}
                  >
                    {cell(row[column])}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
        {table.total > table.rows.length && (
          <div className="px-4 py-1.5 font-mono text-[10px] text-zinc-400">
            +{table.total - table.rows.length} more
          </div>
        )}
      </div>
    </div>
  )
}

function CloseIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      aria-hidden="true"
    >
      <path d="M18 6 6 18M6 6l12 12" />
    </svg>
  )
}
