/**
 * Shared index helpers for this example's services.
 *
 * The api Worker and the indexer daemon both read the shared `index` kv the
 * same way — paging through it with a cursor-following generator, counting the
 * docs, and doing a case-insensitive term search. Those routines live here so
 * the two services share one implementation.
 *
 * This directory sits at the example root, NOT under `services/`, so the
 * pnpm-workspace `services/*` glob never mistakes it for a (package.json-less)
 * workspace member. The structural store type accepts both the SDK's and the
 * bindings package's `Kv` handle.
 */

/** kv key prefix under which every indexed document is stored. */
export const DOC_PREFIX = "doc:"

export interface KvScanPage {
  items: { key: string; value: Uint8Array }[]
  nextCursor?: string
}

export interface KvScanStore {
  scan(prefix: string, limit?: number, cursor?: string): Promise<KvScanPage>
}

/** Iterate every key under a prefix, following the scan cursor across pages. */
export async function* scanAll(store: KvScanStore, prefix: string) {
  let cursor: string | undefined
  do {
    const page = await store.scan(prefix, undefined, cursor)
    for (const item of page.items) yield item
    cursor = page.nextCursor
  } while (cursor)
}

/** Count every document in the shared index. */
export async function countDocs(store: KvScanStore): Promise<number> {
  let count = 0
  for await (const _ of scanAll(store, DOC_PREFIX)) count++
  return count
}

/** Return the ids of documents whose text contains `term` (case-insensitive). */
export async function searchIndex(store: KvScanStore, term: string): Promise<string[]> {
  const hits: string[] = []
  const needle = term.toLowerCase()
  for await (const entry of scanAll(store, DOC_PREFIX)) {
    const text = new TextDecoder().decode(entry.value)
    if (text.toLowerCase().includes(needle)) {
      hits.push(entry.key.slice(DOC_PREFIX.length))
    }
  }
  return hits
}
